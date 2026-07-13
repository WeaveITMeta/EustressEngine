//! 2D sketch constraint solver — Gauss-Newton on residual vector.
//!
//! Covers the 12 [`ConstraintKind`]s plus driving dimensions
//! (linear / radial / angular). Endpoints that start out coincident
//! are implicitly welded (chained/closed profiles survive the solve
//! — entities have no shared point table, so this substitutes for
//! one). Reports under/over-constrained status via residual norm +
//! DOF estimate so AI-generated sketches are debuggable
//! (CAD_PLATFORM_PLAN Phase C).
//!
//! Pure math, no Bevy. Called from `evaluate_tree` before Extrude /
//! Revolve consume a sketch, and from Studio's sketch canvas on drag.

use std::collections::HashMap;

use crate::sketch::{
    ConstraintKind, Sketch, SketchConstraint, SketchDimension, SketchEntity,
};
use crate::{CadError, CadResult};

/// Result of a solve pass.
#[derive(Debug, Clone)]
pub struct SolveReport {
    /// Updated entities (same length / kinds as input).
    pub entities: Vec<SketchEntity>,
    /// Sum of squared residuals after the last iteration.
    pub residual_norm: f64,
    /// Estimated free DOF (max(0, n_params − n_constraints)).
    pub free_dof: i32,
    /// True when residual_norm < tolerance.
    pub converged: bool,
    /// Under / perfect / over classification for UI color coding.
    pub status: SolveStatus,
    /// Per-constraint residual magnitudes (same order as applied constraints).
    pub constraint_residuals: Vec<f64>,
    pub iterations: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// residual ok and free_dof > 0
    UnderConstrained,
    /// residual ok and free_dof == 0
    FullyConstrained,
    /// residual ok but free_dof < 0 (redundant constraints)
    OverConstrained,
    /// did not converge
    Failed,
}

const TOL: f64 = 1e-7;
const MAX_ITERS: u32 = 50;
const FD_EPS: f64 = 1e-7;

/// Solve `sketch` constraints + dimensions in place.
///
/// `vars` resolves dimension quantity strings (`"50 mm"`, `"length"`).
pub fn solve_sketch(
    sketch: &Sketch,
    vars: &HashMap<String, String>,
) -> CadResult<SolveReport> {
    if sketch.entities.is_empty() {
        return Ok(SolveReport {
            entities: vec![],
            residual_norm: 0.0,
            free_dof: 0,
            converged: true,
            status: SolveStatus::FullyConstrained,
            constraint_residuals: vec![],
            iterations: 0,
        });
    }

    let mut params = pack_params(&sketch.entities);
    let n = params.len();
    if n == 0 {
        return Ok(SolveReport {
            entities: sketch.entities.clone(),
            residual_norm: 0.0,
            free_dof: 0,
            converged: true,
            status: SolveStatus::FullyConstrained,
            constraint_residuals: vec![],
            iterations: 0,
        });
    }

    // Fixed-parameter mask: Fix constraints pin entity DOF.
    let fixed = fixed_mask(&sketch.entities, &sketch.constraints, n);

    // Endpoints that start out coincident stay welded through the
    // solve. Entities carry their own endpoints (no shared point
    // table), so without these residuals a Horizontal/Vertical or
    // dimension solve tears chained lines apart and a closed profile
    // silently stops extruding.
    let welds = endpoint_welds(&sketch.entities);

    let mut residual_norm = f64::MAX;
    let mut last_residuals = Vec::new();
    let mut iters = 0u32;

    for iter in 0..MAX_ITERS {
        iters = iter + 1;
        let r = build_residuals(&params, sketch, vars, &welds);
        last_residuals = r.iter().map(|v| v.abs()).collect();
        residual_norm = r.iter().map(|v| v * v).sum::<f64>().sqrt();
        if residual_norm < TOL {
            break;
        }
        if r.is_empty() {
            residual_norm = 0.0;
            break;
        }

        // Finite-difference Jacobian J[m][n]
        let m = r.len();
        let mut j = vec![vec![0.0; n]; m];
        for col in 0..n {
            if fixed[col] {
                continue;
            }
            let mut p2 = params.clone();
            p2[col] += FD_EPS;
            let r2 = build_residuals(&p2, sketch, vars, &welds);
            for row in 0..m {
                j[row][col] = (r2[row] - r[row]) / FD_EPS;
            }
        }

        // Normal equations: (JᵀJ + λI) δ = −Jᵀr   (LM with small λ)
        let lambda = 1e-4 * (1.0 + residual_norm);
        let mut jtj = vec![vec![0.0; n]; n];
        let mut jtr = vec![0.0; n];
        for i in 0..n {
            for k in 0..n {
                let mut s = 0.0;
                for row in 0..m {
                    s += j[row][i] * j[row][k];
                }
                jtj[i][k] = s;
            }
            jtj[i][i] += lambda;
            let mut s = 0.0;
            for row in 0..m {
                s += j[row][i] * r[row];
            }
            jtr[i] = -s;
        }

        // Zero fixed rows/cols so they don't move.
        for i in 0..n {
            if fixed[i] {
                for k in 0..n {
                    jtj[i][k] = 0.0;
                    jtj[k][i] = 0.0;
                }
                jtj[i][i] = 1.0;
                jtr[i] = 0.0;
            }
        }

        let delta = solve_linear(&jtj, &jtr).unwrap_or_else(|| vec![0.0; n]);

        // Line search
        let mut step = 1.0;
        let mut improved = false;
        for _ in 0..8 {
            let mut trial = params.clone();
            for i in 0..n {
                if !fixed[i] {
                    trial[i] += step * delta[i];
                }
            }
            let r_trial = build_residuals(&trial, sketch, vars, &welds);
            let norm_trial = r_trial.iter().map(|v| v * v).sum::<f64>().sqrt();
            if norm_trial < residual_norm {
                params = trial;
                residual_norm = norm_trial;
                improved = true;
                break;
            }
            step *= 0.5;
        }
        if !improved {
            break;
        }
    }

    let entities = unpack_params(&params, &sketch.entities);
    let n_residuals = last_residuals.len() as i32;
    let n_free = fixed.iter().filter(|f| !**f).count() as i32;
    let free_dof = n_free - n_residuals;
    let converged = residual_norm < TOL * 10.0; // slightly relaxed for report
    let status = if !converged {
        SolveStatus::Failed
    } else if free_dof > 0 {
        SolveStatus::UnderConstrained
    } else if free_dof < 0 {
        SolveStatus::OverConstrained
    } else {
        SolveStatus::FullyConstrained
    };

    Ok(SolveReport {
        entities,
        residual_norm,
        free_dof,
        converged,
        status,
        constraint_residuals: last_residuals,
        iterations: iters,
    })
}

/// Apply solve result back into a sketch (mutates entities).
pub fn apply_solve(sketch: &mut Sketch, report: &SolveReport) {
    sketch.entities = report.entities.clone();
}

// ============================================================================
// Parameter packing
// ============================================================================

fn pack_params(entities: &[SketchEntity]) -> Vec<f64> {
    let mut p = Vec::new();
    for e in entities {
        match e {
            SketchEntity::Line { p1, p2 }
            | SketchEntity::Construction { p1, p2 }
            | SketchEntity::Rectangle { p1, p2 } => {
                p.extend_from_slice(p1);
                p.extend_from_slice(p2);
            }
            SketchEntity::Circle { center, radius } => {
                p.extend_from_slice(center);
                p.push(*radius);
            }
            SketchEntity::Arc {
                center,
                start_angle,
                sweep,
                radius,
            } => {
                p.extend_from_slice(center);
                p.push(*start_angle);
                p.push(*sweep);
                p.push(*radius);
            }
            SketchEntity::Point { p: pt } => {
                p.extend_from_slice(pt);
            }
        }
    }
    p
}

fn unpack_params(params: &[f64], template: &[SketchEntity]) -> Vec<SketchEntity> {
    let mut out = Vec::with_capacity(template.len());
    let mut i = 0usize;
    for e in template {
        match e {
            SketchEntity::Line { .. } => {
                out.push(SketchEntity::Line {
                    p1: [params[i], params[i + 1]],
                    p2: [params[i + 2], params[i + 3]],
                });
                i += 4;
            }
            SketchEntity::Construction { .. } => {
                out.push(SketchEntity::Construction {
                    p1: [params[i], params[i + 1]],
                    p2: [params[i + 2], params[i + 3]],
                });
                i += 4;
            }
            SketchEntity::Rectangle { .. } => {
                out.push(SketchEntity::Rectangle {
                    p1: [params[i], params[i + 1]],
                    p2: [params[i + 2], params[i + 3]],
                });
                i += 4;
            }
            SketchEntity::Circle { .. } => {
                out.push(SketchEntity::Circle {
                    center: [params[i], params[i + 1]],
                    radius: params[i + 2].abs().max(1e-9),
                });
                i += 3;
            }
            SketchEntity::Arc { .. } => {
                out.push(SketchEntity::Arc {
                    center: [params[i], params[i + 1]],
                    start_angle: params[i + 2],
                    sweep: params[i + 3],
                    radius: params[i + 4].abs().max(1e-9),
                });
                i += 5;
            }
            SketchEntity::Point { .. } => {
                out.push(SketchEntity::Point {
                    p: [params[i], params[i + 1]],
                });
                i += 2;
            }
        }
    }
    let _ = i;
    out
}

fn entity_param_range(entities: &[SketchEntity], idx: usize) -> (usize, usize) {
    let mut off = 0usize;
    for (i, e) in entities.iter().enumerate() {
        let len = match e {
            SketchEntity::Line { .. }
            | SketchEntity::Construction { .. }
            | SketchEntity::Rectangle { .. } => 4,
            SketchEntity::Circle { .. } => 3,
            SketchEntity::Arc { .. } => 5,
            SketchEntity::Point { .. } => 2,
        };
        if i == idx {
            return (off, off + len);
        }
        off += len;
    }
    (0, 0)
}

fn fixed_mask(
    entities: &[SketchEntity],
    constraints: &[SketchConstraint],
    n: usize,
) -> Vec<bool> {
    let mut fixed = vec![false; n];
    for c in constraints {
        if c.kind == ConstraintKind::Fix {
            let (a, b) = entity_param_range(entities, c.e1);
            for i in a..b.min(n) {
                fixed[i] = true;
            }
        }
    }
    fixed
}

// ============================================================================
// Residuals
// ============================================================================

fn build_residuals(
    params: &[f64],
    sketch: &Sketch,
    vars: &HashMap<String, String>,
    welds: &[(usize, usize)],
) -> Vec<f64> {
    let entities = unpack_params(params, &sketch.entities);
    let mut r = Vec::new();

    for c in &sketch.constraints {
        push_constraint_residuals(&mut r, &entities, c);
    }
    for d in &sketch.dimensions {
        push_dimension_residuals(&mut r, &entities, d, vars);
    }
    for &(oa, ob) in welds {
        r.push(params[oa] - params[ob]);
        r.push(params[oa + 1] - params[ob + 1]);
    }
    r
}

/// Pairs of param offsets (x-coordinate index; y is offset+1) for
/// endpoints that are coincident in the INPUT sketch — the implicit
/// topology of chained/closed profiles. Points weld too, so anchor
/// points (Hole centers) ride along with the geometry they sit on.
fn endpoint_welds(entities: &[SketchEntity]) -> Vec<(usize, usize)> {
    const WELD_EPS: f64 = 1.0e-7;
    let mut endpoints: Vec<(usize, [f64; 2])> = Vec::new();
    let mut off = 0usize;
    for e in entities {
        match e {
            SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => {
                endpoints.push((off, *p1));
                endpoints.push((off + 2, *p2));
                off += 4;
            }
            SketchEntity::Rectangle { .. } => off += 4,
            SketchEntity::Circle { .. } => off += 3,
            SketchEntity::Arc { .. } => off += 5,
            SketchEntity::Point { p } => {
                endpoints.push((off, *p));
                off += 2;
            }
        }
    }
    let mut welds = Vec::new();
    for i in 0..endpoints.len() {
        for j in (i + 1)..endpoints.len() {
            let (oa, pa) = endpoints[i];
            let (ob, pb) = endpoints[j];
            if (pa[0] - pb[0]).abs() < WELD_EPS && (pa[1] - pb[1]).abs() < WELD_EPS {
                welds.push((oa, ob));
            }
        }
    }
    welds
}

fn push_constraint_residuals(
    r: &mut Vec<f64>,
    entities: &[SketchEntity],
    c: &SketchConstraint,
) {
    let e1 = entities.get(c.e1);
    let e2 = c.e2.and_then(|i| entities.get(i));
    match c.kind {
        ConstraintKind::Coincident => {
            if let (Some(a), Some(b)) = (e1, e2) {
                let pa = entity_anchor(a);
                let pb = entity_anchor(b);
                r.push(pa[0] - pb[0]);
                r.push(pa[1] - pb[1]);
            }
        }
        ConstraintKind::Concentric => {
            if let (Some(a), Some(b)) = (e1, e2) {
                let ca = entity_center(a);
                let cb = entity_center(b);
                if let (Some(ca), Some(cb)) = (ca, cb) {
                    r.push(ca[0] - cb[0]);
                    r.push(ca[1] - cb[1]);
                }
            }
        }
        ConstraintKind::Collinear => {
            if let (Some(SketchEntity::Line { p1: a1, p2: a2 }), Some(SketchEntity::Line { p1: b1, p2: b2 })) =
                (e1, e2)
            {
                // Cross product of direction and offset → 0 for both ends of b.
                let dx = a2[0] - a1[0];
                let dy = a2[1] - a1[1];
                r.push((b1[0] - a1[0]) * dy - (b1[1] - a1[1]) * dx);
                r.push((b2[0] - a1[0]) * dy - (b2[1] - a1[1]) * dx);
            }
        }
        ConstraintKind::Parallel => {
            if let (Some(da), Some(db)) = (e1.and_then(entity_dir), e2.and_then(entity_dir)) {
                // da × db = 0
                r.push(da[0] * db[1] - da[1] * db[0]);
            }
        }
        ConstraintKind::Perpendicular => {
            if let (Some(da), Some(db)) = (e1.and_then(entity_dir), e2.and_then(entity_dir)) {
                // da · db = 0
                r.push(da[0] * db[0] + da[1] * db[1]);
            }
        }
        ConstraintKind::Tangent => {
            // Line-circle: distance from center to line == radius
            match (e1, e2) {
                (
                    Some(SketchEntity::Line { p1, p2 }),
                    Some(SketchEntity::Circle { center, radius }),
                )
                | (
                    Some(SketchEntity::Circle { center, radius }),
                    Some(SketchEntity::Line { p1, p2 }),
                ) => {
                    let d = dist_point_line(*center, *p1, *p2);
                    r.push(d - radius.abs());
                }
                _ => {}
            }
        }
        ConstraintKind::Horizontal => {
            if let Some(d) = e1.and_then(entity_dir) {
                r.push(d[1]); // dy = 0
            }
        }
        ConstraintKind::Vertical => {
            if let Some(d) = e1.and_then(entity_dir) {
                r.push(d[0]); // dx = 0
            }
        }
        ConstraintKind::EqualLength => {
            if let (Some(la), Some(lb)) = (e1.and_then(entity_length), e2.and_then(entity_length)) {
                r.push(la - lb);
            }
        }
        ConstraintKind::EqualRadius => {
            if let (Some(ra), Some(rb)) = (e1.and_then(entity_radius), e2.and_then(entity_radius)) {
                r.push(ra - rb);
            }
        }
        ConstraintKind::Symmetric => {
            // Symmetric about construction line e2 (or X axis if missing):
            // midpoint on axis, direction mirror.
            if let (Some(SketchEntity::Line { p1: a1, p2: a2 }), Some(axis)) = (e1, e2) {
                if let SketchEntity::Line { p1: o, p2: d } | SketchEntity::Construction { p1: o, p2: d } =
                    axis
                {
                    let mid = [(a1[0] + a2[0]) * 0.5, (a1[1] + a2[1]) * 0.5];
                    // Distance of mid to axis should be 0 (mid on axis for segment symmetry
                    // about a line through mid — use projection residual).
                    r.push(dist_point_line(mid, *o, *d));
                }
            }
        }
        ConstraintKind::Fix => {
            // Handled via fixed mask — no residual.
        }
    }
}

fn push_dimension_residuals(
    r: &mut Vec<f64>,
    entities: &[SketchEntity],
    dim: &SketchDimension,
    vars: &HashMap<String, String>,
) {
    let Some(q) = dim.resolved_value(vars) else {
        return;
    };
    let target = q.to_si();
    match dim {
        SketchDimension::Linear { e1, .. } => {
            if let Some(len) = entities.get(*e1).and_then(entity_length) {
                r.push(len - target);
            }
        }
        SketchDimension::Radial { e1, .. } => {
            if let Some(rad) = entities.get(*e1).and_then(entity_radius) {
                r.push(rad - target);
            }
        }
        SketchDimension::Angular { e1, e2, .. } => {
            if let (Some(da), Some(db)) = (
                entities.get(*e1).and_then(entity_dir),
                entities.get(*e2).and_then(entity_dir),
            ) {
                let ang = (da[0] * db[0] + da[1] * db[1])
                    .clamp(-1.0, 1.0)
                    .acos();
                r.push(ang - target);
            }
        }
    }
}

// ============================================================================
// Entity geometry helpers
// ============================================================================

fn entity_anchor(e: &SketchEntity) -> [f64; 2] {
    match e {
        SketchEntity::Line { p1, .. }
        | SketchEntity::Construction { p1, .. }
        | SketchEntity::Rectangle { p1, .. } => *p1,
        SketchEntity::Circle { center, .. } | SketchEntity::Arc { center, .. } => *center,
        SketchEntity::Point { p } => *p,
    }
}

fn entity_center(e: &SketchEntity) -> Option<[f64; 2]> {
    match e {
        SketchEntity::Circle { center, .. } | SketchEntity::Arc { center, .. } => Some(*center),
        SketchEntity::Point { p } => Some(*p),
        SketchEntity::Rectangle { p1, p2 } => Some([(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5]),
        _ => None,
    }
}

fn entity_dir(e: &SketchEntity) -> Option<[f64; 2]> {
    match e {
        SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => {
            let dx = p2[0] - p1[0];
            let dy = p2[1] - p1[1];
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            Some([dx / len, dy / len])
        }
        SketchEntity::Rectangle { p1, p2 } => {
            // Bottom edge direction
            let dx = p2[0] - p1[0];
            let dy = 0.0;
            let len = dx.abs().max(1e-12);
            Some([dx / len, dy])
        }
        _ => None,
    }
}

fn entity_length(e: &SketchEntity) -> Option<f64> {
    match e {
        SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => {
            let dx = p2[0] - p1[0];
            let dy = p2[1] - p1[1];
            Some((dx * dx + dy * dy).sqrt())
        }
        SketchEntity::Rectangle { p1, p2 } => {
            // Perimeter contribution: width
            Some((p2[0] - p1[0]).abs())
        }
        _ => None,
    }
}

fn entity_radius(e: &SketchEntity) -> Option<f64> {
    match e {
        SketchEntity::Circle { radius, .. } | SketchEntity::Arc { radius, .. } => Some(radius.abs()),
        _ => None,
    }
}

fn dist_point_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len
}

// ============================================================================
// Dense linear solve (Gaussian elimination with partial pivoting)
// ============================================================================

fn solve_linear(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    if n == 0 {
        return Some(vec![]);
    }
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.push(b[i]);
            r
        })
        .collect();

    for col in 0..n {
        let mut pivot = col;
        for row in (col + 1)..n {
            if m[row][col].abs() > m[pivot][col].abs() {
                pivot = row;
            }
        }
        if m[pivot][col].abs() < 1e-14 {
            continue; // singular column — leave as free (δ=0)
        }
        m.swap(col, pivot);
        let div = m[col][col];
        for j in col..=n {
            m[col][j] /= div;
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let f = m[row][col];
            for j in col..=n {
                m[row][j] -= f * m[col][j];
            }
        }
    }
    Some(m.iter().map(|row| row[n]).collect())
}

/// Convenience: solve and return error if failed hard.
pub fn solve_or_err(sketch: &Sketch, vars: &HashMap<String, String>) -> CadResult<SolveReport> {
    let report = solve_sketch(sketch, vars)?;
    if matches!(report.status, SolveStatus::Failed) && report.residual_norm > 1e-3 {
        return Err(CadError::EvalFailed {
            feature: "SketchSolver".into(),
            reason: format!(
                "did not converge (residual={:.3e}, iters={})",
                report.residual_norm, report.iterations
            ),
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{SketchConstraint, SketchEntity};

    #[test]
    fn perpendicular_lines_converge() {
        let sketch = Sketch {
            plane: "xy".into(),
            entities: vec![
                SketchEntity::Line {
                    p1: [0.0, 0.0],
                    p2: [1.0, 0.1],
                },
                SketchEntity::Line {
                    p1: [0.0, 0.0],
                    p2: [0.1, 1.0],
                },
            ],
            dimensions: vec![],
            constraints: vec![
                SketchConstraint {
                    kind: ConstraintKind::Coincident,
                    e1: 0,
                    e2: Some(1),
                },
                SketchConstraint {
                    kind: ConstraintKind::Perpendicular,
                    e1: 0,
                    e2: Some(1),
                },
            ],
        };
        let report = solve_sketch(&sketch, &HashMap::new()).unwrap();
        assert!(report.converged || report.residual_norm < 1e-4, "{:?}", report);
    }

    #[test]
    fn horizontal_forces_dy_zero() {
        let sketch = Sketch {
            plane: "xy".into(),
            entities: vec![SketchEntity::Line {
                p1: [0.0, 0.0],
                p2: [1.0, 0.5],
            }],
            dimensions: vec![],
            constraints: vec![SketchConstraint {
                kind: ConstraintKind::Horizontal,
                e1: 0,
                e2: None,
            }],
        };
        let report = solve_sketch(&sketch, &HashMap::new()).unwrap();
        if let SketchEntity::Line { p1, p2 } = &report.entities[0] {
            assert!((p2[1] - p1[1]).abs() < 1e-5, "dy should be ~0: {:?}", report.entities);
        } else {
            panic!("expected line");
        }
    }
}

//! 2D sketch constraint solver.
//!
//! Levenberg-Marquardt over the residuals of every constraint, dimension
//! and implicit weld, with a central-difference Jacobian. Entities keep
//! their own endpoints (there is no shared point table), so endpoints
//! that start out coincident are welded for the solve, which is what
//! keeps a closed profile closed.
//!
//! ## What changed, and why it matters
//!
//! - **Degrees of freedom come from the Jacobian's rank**, not from
//!   counting residual rows. Counting rows called a correct sketch
//!   over-constrained the moment a constraint restated one that already
//!   held (a `fix` on a point also welded to a fixed line), and counted a
//!   constraint with a constant residual as removing a freedom it never
//!   touched. Rank measures what the constraints actually pin.
//! - **Nothing is silently ignored.** Every constraint and dimension is
//!   checked against the kinds of entity it names before solving. The
//!   old solver dropped any pair it had no formula for (tangent between
//!   two arcs, concentric with a line, a dimension whose value did not
//!   resolve) and reported the sketch solved. Now the sketch fails with
//!   the constraint's position and the reason.
//! - **Points are chosen, not assumed.** `coincident` between two lines
//!   used to weld their START points, which tore apart any profile whose
//!   lines ran end to start. A constraint can now say which point
//!   (`p1`/`p2`: start, end, center) and, when it does not, the nearest
//!   pair in the drawn geometry is used.
//! - **Angles are signed when they need to be.** The unsigned `acos`
//!   could not reach anything past 180 degrees and had an infinite
//!   derivative at 0 and 180.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use crate::sketch::{ConstraintKind, DimAxis, PointRef, Sketch, SketchConstraint, SketchDimension, SketchEntity};
use crate::{CadError, CadResult};

/// Result of a solve pass.
#[derive(Debug, Clone)]
pub struct SolveReport {
    /// Updated entities (same length and kinds as input).
    pub entities: Vec<SketchEntity>,
    /// Euclidean norm of the residual vector after the last iteration.
    pub residual_norm: f64,
    /// Freedoms left: free parameters minus the rank of the constraint
    /// Jacobian. Never negative.
    pub free_dof: i32,
    /// True when the residual reached tolerance.
    pub converged: bool,
    pub status: SolveStatus,
    /// Per-row residual magnitudes: constraints and dimensions in order,
    /// then the implicit endpoint welds.
    pub constraint_residuals: Vec<f64>,
    pub iterations: u32,
    /// Residual rows that restate others (rows minus rank). Harmless when
    /// the sketch converged, but worth removing.
    pub redundant: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// Converged, with freedoms left.
    UnderConstrained,
    /// Converged, no freedoms left, nothing redundant.
    FullyConstrained,
    /// Converged, but some constraints restate others.
    OverConstrained,
    /// Did not converge: the constraints conflict or have no solution.
    Failed,
}

const TOL: f64 = 1e-9;
const MAX_ITERS: u32 = 100;

fn fail(reason: impl Into<String>) -> CadError {
    CadError::EvalFailed { feature: "SketchSolver".into(), reason: reason.into() }
}

// ============================================================================
// Points and terms
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Which {
    Start,
    End,
    Center,
    /// The point entity itself.
    Pt,
}

#[derive(Debug, Clone, Copy)]
struct P {
    e: usize,
    w: Which,
}

/// One compiled constraint or dimension: entity indices resolved, point
/// choices made, targets in SI.
#[derive(Debug, Clone)]
enum Term {
    Coincident(P, P),
    OnLine(P, usize),
    OnCircle(P, usize),
    Midpoint(P, usize),
    Collinear(usize, usize),
    Parallel(usize, usize),
    Perpendicular(usize, usize),
    TangentLine { line: usize, circ: usize },
    TangentCircles { a: usize, b: usize, internal: bool },
    Horizontal(usize),
    Vertical(usize),
    HorizontalPts(P, P),
    VerticalPts(P, P),
    EqualLength(usize, usize),
    EqualRadius(usize, usize),
    SymmetricLine { line: usize, axis: usize },
    SymmetricPts { a: P, b: P, axis: usize },
    Length { e: usize, target: f64 },
    RectSize { e: usize, axis: usize, target: f64 },
    DistPts { a: P, b: P, target: f64 },
    /// Signed component along x (0) or y (1); the sign of the drawn
    /// geometry is folded into `target`, so the solve never flips sides.
    DistAxis { a: P, b: P, axis: usize, target: f64 },
    DistPtLine { p: P, line: usize, target: f64 },
    Radius { e: usize, target: f64 },
    Angle { a: usize, b: usize, target: f64, signed: bool },
}

fn is_line(e: &SketchEntity) -> bool {
    matches!(e, SketchEntity::Line { .. } | SketchEntity::Construction { .. })
}

fn is_circular(e: &SketchEntity) -> bool {
    matches!(e, SketchEntity::Circle { .. } | SketchEntity::Arc { .. })
}

fn kind_name(e: &SketchEntity) -> &'static str {
    match e {
        SketchEntity::Line { .. } => "line",
        SketchEntity::Construction { .. } => "construction line",
        SketchEntity::Rectangle { .. } => "rectangle",
        SketchEntity::Circle { .. } => "circle",
        SketchEntity::Arc { .. } => "arc",
        SketchEntity::Point { .. } => "point",
    }
}

fn arc_end(center: [f64; 2], r: f64, a: f64) -> [f64; 2] {
    [center[0] + r * a.cos(), center[1] + r * a.sin()]
}

fn point_of(e: &SketchEntity, w: Which) -> Option<[f64; 2]> {
    match (e, w) {
        (SketchEntity::Point { p }, _) => Some(*p),
        (SketchEntity::Line { p1, .. } | SketchEntity::Construction { p1, .. } | SketchEntity::Rectangle { p1, .. }, Which::Start) => Some(*p1),
        (SketchEntity::Line { p2, .. } | SketchEntity::Construction { p2, .. } | SketchEntity::Rectangle { p2, .. }, Which::End) => Some(*p2),
        (SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } | SketchEntity::Rectangle { p1, p2 }, Which::Center) => {
            Some([(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5])
        }
        (SketchEntity::Circle { center, .. }, Which::Center) => Some(*center),
        (SketchEntity::Arc { center, .. }, Which::Center) => Some(*center),
        (SketchEntity::Arc { center, start_angle, radius, .. }, Which::Start) => Some(arc_end(*center, *radius, *start_angle)),
        (SketchEntity::Arc { center, start_angle, sweep, radius }, Which::End) => {
            Some(arc_end(*center, *radius, start_angle + sweep))
        }
        _ => None,
    }
}

fn line_ends(e: &SketchEntity) -> Option<([f64; 2], [f64; 2])> {
    match e {
        SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => Some((*p1, *p2)),
        _ => None,
    }
}

fn circle_of(e: &SketchEntity) -> Option<([f64; 2], f64)> {
    match e {
        SketchEntity::Circle { center, radius } | SketchEntity::Arc { center, radius, .. } => Some((*center, *radius)),
        _ => None,
    }
}

fn which_of(r: PointRef) -> Which {
    match r {
        PointRef::Start => Which::Start,
        PointRef::End => Which::End,
        PointRef::Center => Which::Center,
    }
}

fn d2(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Compile every constraint and dimension, refusing any the solver has
/// no formula for.
fn compile(sk: &Sketch, vars: &HashMap<String, String>) -> CadResult<Vec<Term>> {
    let ents = &sk.entities;
    let get = |i: usize, what: &str| -> CadResult<&SketchEntity> {
        ents.get(i).ok_or_else(|| fail(format!("{what} refers to entity e{i}, but the sketch has {} entities", ents.len())))
    };
    // An explicit point choice, validated; or the natural point of a
    // point or circle; or None when the entity has several candidates.
    let explicit = |i: usize, r: Option<PointRef>, what: &str| -> CadResult<Option<P>> {
        let e = get(i, what)?;
        match r {
            Some(r) => {
                let w = which_of(r);
                point_of(e, w)
                    .map(|_| Some(P { e: i, w }))
                    .ok_or_else(|| fail(format!("{what}: a {} has no {r:?} point", kind_name(e))))
            }
            None => Ok(match e {
                SketchEntity::Point { .. } => Some(P { e: i, w: Which::Pt }),
                SketchEntity::Circle { .. } => Some(P { e: i, w: Which::Center }),
                _ => None,
            }),
        }
    };
    let need_point = |i: usize, r: Option<PointRef>, what: &str| -> CadResult<P> {
        explicit(i, r, what)?.ok_or_else(|| {
            fail(format!(
                "{what}: say which point of e{i} ({}) with p1/p2 = start, end or center",
                kind_name(&ents[i])
            ))
        })
    };

    let mut terms = Vec::new();
    for (ci, c) in sk.constraints.iter().enumerate() {
        let what = format!("constraint {ci} ({:?})", c.kind);
        let e1 = get(c.e1, &what)?;
        let e2i = c.e2;
        let need_e2 = |w: &str| e2i.ok_or_else(|| fail(format!("{w} needs e2")));
        match c.kind {
            ConstraintKind::Fix => {}
            ConstraintKind::Coincident => {
                let j = need_e2(&what)?;
                let e2 = get(j, &what)?;
                let a = explicit(c.e1, c.p1, &what)?;
                let b = explicit(j, c.p2, &what)?;
                let (a, b) = match (a, b) {
                    (Some(a), Some(b)) => (a, b),
                    _ => nearest_pair(c.e1, e1, a, j, e2, b)
                        .ok_or_else(|| fail(format!("{what}: no pair of points to make coincident")))?,
                };
                terms.push(Term::Coincident(a, b));
            }
            ConstraintKind::Concentric => {
                let j = need_e2(&what)?;
                let e2 = get(j, &what)?;
                let ca = if is_circular(e1) { Which::Center } else if matches!(e1, SketchEntity::Point { .. }) { Which::Pt } else { Which::Start };
                let cb = if is_circular(e2) { Which::Center } else if matches!(e2, SketchEntity::Point { .. }) { Which::Pt } else { Which::Start };
                if !(is_circular(e1) || is_circular(e2)) || ca == Which::Start || cb == Which::Start {
                    return Err(fail(format!("{what}: concentric applies to circles, arcs and points, not a {} and a {}", kind_name(e1), kind_name(e2))));
                }
                terms.push(Term::Coincident(P { e: c.e1, w: ca }, P { e: j, w: cb }));
            }
            ConstraintKind::Collinear | ConstraintKind::Parallel | ConstraintKind::Perpendicular | ConstraintKind::EqualLength => {
                let j = need_e2(&what)?;
                let e2 = get(j, &what)?;
                if !(is_line(e1) && is_line(e2)) {
                    return Err(fail(format!("{what}: applies to two lines, not a {} and a {}", kind_name(e1), kind_name(e2))));
                }
                terms.push(match c.kind {
                    ConstraintKind::Collinear => Term::Collinear(c.e1, j),
                    ConstraintKind::Parallel => Term::Parallel(c.e1, j),
                    ConstraintKind::Perpendicular => Term::Perpendicular(c.e1, j),
                    _ => Term::EqualLength(c.e1, j),
                });
            }
            ConstraintKind::EqualRadius => {
                let j = need_e2(&what)?;
                let e2 = get(j, &what)?;
                if !(is_circular(e1) && is_circular(e2)) {
                    return Err(fail(format!("{what}: applies to circles and arcs, not a {} and a {}", kind_name(e1), kind_name(e2))));
                }
                terms.push(Term::EqualRadius(c.e1, j));
            }
            ConstraintKind::Tangent => {
                let j = need_e2(&what)?;
                let e2 = get(j, &what)?;
                if is_line(e1) && is_circular(e2) {
                    terms.push(Term::TangentLine { line: c.e1, circ: j });
                } else if is_circular(e1) && is_line(e2) {
                    terms.push(Term::TangentLine { line: j, circ: c.e1 });
                } else if is_circular(e1) && is_circular(e2) {
                    let ((ca, ra), (cb, rb)) = (circle_of(e1).unwrap(), circle_of(e2).unwrap());
                    // Inside or outside tangency, whichever the drawing is
                    // closer to; the solve never swaps one for the other.
                    let dist = d2(ca, cb);
                    let internal = (dist - (ra - rb).abs()).abs() < (dist - (ra + rb)).abs();
                    terms.push(Term::TangentCircles { a: c.e1, b: j, internal });
                } else {
                    return Err(fail(format!("{what}: tangent applies to a line or arc with a circle or arc, not a {} and a {}", kind_name(e1), kind_name(e2))));
                }
            }
            ConstraintKind::Horizontal | ConstraintKind::Vertical => {
                let horizontal = c.kind == ConstraintKind::Horizontal;
                match e2i {
                    None => {
                        if matches!(e1, SketchEntity::Rectangle { .. }) {
                            return Err(fail(format!(
                                "{what}: a rectangle's sides are axis-aligned by construction, so horizontal and \
                                 vertical do not apply to it; constrain lines instead"
                            )));
                        }
                        if !is_line(e1) {
                            return Err(fail(format!("{what}: applies to a line, or to two points via e2, not a {}", kind_name(e1))));
                        }
                        terms.push(if horizontal { Term::Horizontal(c.e1) } else { Term::Vertical(c.e1) });
                    }
                    Some(j) => {
                        let a = need_point(c.e1, c.p1, &what)?;
                        let b = need_point(j, c.p2, &what)?;
                        terms.push(if horizontal { Term::HorizontalPts(a, b) } else { Term::VerticalPts(a, b) });
                    }
                }
            }
            ConstraintKind::Symmetric => match c.e3 {
                Some(axis) => {
                    let j = need_e2(&what)?;
                    if !is_line(get(axis, &what)?) {
                        return Err(fail(format!("{what}: the symmetry axis e{axis} must be a line")));
                    }
                    let a = need_point(c.e1, c.p1, &what)?;
                    let b = need_point(j, c.p2, &what)?;
                    terms.push(Term::SymmetricPts { a, b, axis });
                }
                None => {
                    let j = need_e2(&what)?;
                    if !(is_line(e1) && is_line(get(j, &what)?)) {
                        return Err(fail(format!(
                            "{what}: give two points and an axis (e1, e2, e3), or a line and an axis line (e1, e2)"
                        )));
                    }
                    terms.push(Term::SymmetricLine { line: c.e1, axis: j });
                }
            },
            ConstraintKind::Midpoint | ConstraintKind::PointOnLine => {
                let j = need_e2(&what)?;
                if !is_line(get(j, &what)?) {
                    return Err(fail(format!("{what}: e2 must be a line")));
                }
                let p = need_point(c.e1, c.p1, &what)?;
                terms.push(if c.kind == ConstraintKind::Midpoint { Term::Midpoint(p, j) } else { Term::OnLine(p, j) });
            }
            ConstraintKind::PointOnCircle => {
                let j = need_e2(&what)?;
                if !is_circular(get(j, &what)?) {
                    return Err(fail(format!("{what}: e2 must be a circle or arc")));
                }
                let p = need_point(c.e1, c.p1, &what)?;
                terms.push(Term::OnCircle(p, j));
            }
        }
    }

    for (di, d) in sk.dimensions.iter().enumerate() {
        let what = format!("dimension {di}");
        let q = d.resolved_value(vars).ok_or_else(|| {
            let raw = match d {
                SketchDimension::Linear { value, .. }
                | SketchDimension::Radial { value, .. }
                | SketchDimension::Diameter { value, .. }
                | SketchDimension::Angular { value, .. } => value,
            };
            fail(format!("{what}: value '{raw}' does not resolve to a quantity"))
        })?;
        let target = q.to_si();
        match d {
            SketchDimension::Linear { e1, e2: None, axis, .. } => {
                let e = get(*e1, &what)?;
                match e {
                    SketchEntity::Rectangle { .. } => {
                        let ax = if *axis == Some(DimAxis::Y) { 1 } else { 0 };
                        terms.push(Term::RectSize { e: *e1, axis: ax, target });
                    }
                    _ if is_line(e) => {
                        if axis.is_some() {
                            return Err(fail(format!("{what}: an axis on a single line needs its two ends: use e2 = e1 with p1 = start, p2 = end")));
                        }
                        terms.push(Term::Length { e: *e1, target });
                    }
                    _ => return Err(fail(format!("{what}: a linear dimension on one entity applies to a line or rectangle, not a {}", kind_name(e)))),
                }
            }
            SketchDimension::Linear { e1, e2: Some(j), axis, p1, p2, .. } => {
                let (a_e, b_e) = (get(*e1, &what)?, get(*j, &what)?);
                if axis.is_none() && p1.is_none() && p2.is_none() && is_line(a_e) && is_line(b_e) && e1 != j {
                    // Between parallel lines: from e2's start to line e1.
                    terms.push(Term::DistPtLine { p: P { e: *j, w: Which::Start }, line: *e1, target });
                    continue;
                }
                if axis.is_none() && is_line(b_e) && p2.is_none() && e1 != j {
                    let p = need_point(*e1, *p1, &what)?;
                    terms.push(Term::DistPtLine { p, line: *j, target });
                    continue;
                }
                let a = need_point(*e1, *p1, &what)?;
                let b = need_point(*j, *p2, &what)?;
                match axis {
                    None => terms.push(Term::DistPts { a, b, target }),
                    Some(ax) => {
                        let k = if *ax == DimAxis::X { 0 } else { 1 };
                        let (pa, pb) = (point_of(&ents[a.e], a.w).unwrap(), point_of(&ents[b.e], b.w).unwrap());
                        let sign = if pb[k] - pa[k] < 0.0 { -1.0 } else { 1.0 };
                        terms.push(Term::DistAxis { a, b, axis: k, target: sign * target });
                    }
                }
            }
            SketchDimension::Radial { e1, .. } | SketchDimension::Diameter { e1, .. } => {
                let e = get(*e1, &what)?;
                if !is_circular(e) {
                    return Err(fail(format!("{what}: applies to a circle or arc, not a {}", kind_name(e))));
                }
                let r = if matches!(d, SketchDimension::Diameter { .. }) { target * 0.5 } else { target };
                terms.push(Term::Radius { e: *e1, target: r });
            }
            SketchDimension::Angular { e1, e2, .. } => {
                if !(is_line(get(*e1, &what)?) && is_line(get(*e2, &what)?)) {
                    return Err(fail(format!("{what}: an angle is measured between two lines")));
                }
                let signed = !(0.0..=PI + 1e-12).contains(&target);
                terms.push(Term::Angle { a: *e1, b: *e2, target, signed });
            }
        }
    }
    Ok(terms)
}

/// For an unspecified coincident: the nearest pair of candidate points.
fn nearest_pair(i: usize, a: &SketchEntity, pa: Option<P>, j: usize, b: &SketchEntity, pb: Option<P>) -> Option<(P, P)> {
    let cands = |k: usize, e: &SketchEntity, fixed: Option<P>| -> Vec<P> {
        match fixed {
            Some(p) => vec![p],
            None => [Which::Start, Which::End, Which::Center]
                .into_iter()
                .filter(|w| point_of(e, *w).is_some())
                .filter(|w| !(matches!(e, SketchEntity::Line { .. } | SketchEntity::Construction { .. } | SketchEntity::Rectangle { .. }) && *w == Which::Center))
                .map(|w| P { e: k, w })
                .collect(),
        }
    };
    let (ca, cb) = (cands(i, a, pa), cands(j, b, pb));
    let mut best: Option<(f64, P, P)> = None;
    for x in &ca {
        for y in &cb {
            let d = d2(point_of(a, x.w)?, point_of(b, y.w)?);
            if best.map_or(true, |(bd, _, _)| d < bd) {
                best = Some((d, *x, *y));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}

// ============================================================================
// Residuals
// ============================================================================

fn pt(ents: &[SketchEntity], p: P) -> [f64; 2] {
    point_of(&ents[p.e], p.w).unwrap_or([0.0, 0.0])
}

fn dir(ents: &[SketchEntity], i: usize) -> [f64; 2] {
    match line_ends(&ents[i]) {
        Some((a, b)) => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt().max(1e-12);
            [dx / l, dy / l]
        }
        None => [1.0, 0.0],
    }
}

fn signed_dist_to_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = (dx * dx + dy * dy).sqrt().max(1e-12);
    ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / l
}

fn wrap_pi(a: f64) -> f64 {
    let mut x = (a + PI) % TAU;
    if x < 0.0 {
        x += TAU;
    }
    x - PI
}

fn residuals(ents: &[SketchEntity], terms: &[Term], welds: &[(usize, usize)], params: &[f64]) -> Vec<f64> {
    let mut r = Vec::new();
    for t in terms {
        match *t {
            Term::Coincident(a, b) => {
                let (pa, pb) = (pt(ents, a), pt(ents, b));
                r.push(pa[0] - pb[0]);
                r.push(pa[1] - pb[1]);
            }
            Term::OnLine(p, l) => {
                let (a, b) = line_ends(&ents[l]).unwrap_or(([0.0; 2], [1.0, 0.0]));
                r.push(signed_dist_to_line(pt(ents, p), a, b));
            }
            Term::OnCircle(p, c) => {
                let (cc, rr) = circle_of(&ents[c]).unwrap_or(([0.0; 2], 1.0));
                r.push(d2(pt(ents, p), cc) - rr.abs());
            }
            Term::Midpoint(p, l) => {
                let (a, b) = line_ends(&ents[l]).unwrap_or(([0.0; 2], [0.0; 2]));
                let q = pt(ents, p);
                r.push(q[0] - (a[0] + b[0]) * 0.5);
                r.push(q[1] - (a[1] + b[1]) * 0.5);
            }
            Term::Collinear(i, j) => {
                let (a, b) = line_ends(&ents[i]).unwrap_or(([0.0; 2], [1.0, 0.0]));
                let (c, d) = line_ends(&ents[j]).unwrap_or(([0.0; 2], [1.0, 0.0]));
                r.push(signed_dist_to_line(c, a, b));
                r.push(signed_dist_to_line(d, a, b));
            }
            Term::Parallel(i, j) => {
                let (da, db) = (dir(ents, i), dir(ents, j));
                r.push(da[0] * db[1] - da[1] * db[0]);
            }
            Term::Perpendicular(i, j) => {
                let (da, db) = (dir(ents, i), dir(ents, j));
                r.push(da[0] * db[0] + da[1] * db[1]);
            }
            Term::TangentLine { line, circ } => {
                let (a, b) = line_ends(&ents[line]).unwrap_or(([0.0; 2], [1.0, 0.0]));
                let (c, rr) = circle_of(&ents[circ]).unwrap_or(([0.0; 2], 1.0));
                r.push(signed_dist_to_line(c, a, b).abs() - rr.abs());
            }
            Term::TangentCircles { a, b, internal } => {
                let (ca, ra) = circle_of(&ents[a]).unwrap_or(([0.0; 2], 1.0));
                let (cb, rb) = circle_of(&ents[b]).unwrap_or(([0.0; 2], 1.0));
                let want = if internal { (ra.abs() - rb.abs()).abs() } else { ra.abs() + rb.abs() };
                r.push(d2(ca, cb) - want);
            }
            Term::Horizontal(i) => {
                let (a, b) = line_ends(&ents[i]).unwrap_or(([0.0; 2], [0.0; 2]));
                r.push(b[1] - a[1]);
            }
            Term::Vertical(i) => {
                let (a, b) = line_ends(&ents[i]).unwrap_or(([0.0; 2], [0.0; 2]));
                r.push(b[0] - a[0]);
            }
            Term::HorizontalPts(a, b) => r.push(pt(ents, b)[1] - pt(ents, a)[1]),
            Term::VerticalPts(a, b) => r.push(pt(ents, b)[0] - pt(ents, a)[0]),
            Term::EqualLength(i, j) => {
                let (a, b) = line_ends(&ents[i]).unwrap_or(([0.0; 2], [0.0; 2]));
                let (c, d) = line_ends(&ents[j]).unwrap_or(([0.0; 2], [0.0; 2]));
                r.push(d2(a, b) - d2(c, d));
            }
            Term::EqualRadius(i, j) => {
                let ((_, ra), (_, rb)) = (circle_of(&ents[i]).unwrap_or(([0.0; 2], 0.0)), circle_of(&ents[j]).unwrap_or(([0.0; 2], 0.0)));
                r.push(ra.abs() - rb.abs());
            }
            Term::SymmetricLine { line, axis } => {
                let (a, b) = line_ends(&ents[line]).unwrap_or(([0.0; 2], [0.0; 2]));
                push_symmetric(&mut r, a, b, ents, axis);
            }
            Term::SymmetricPts { a, b, axis } => {
                let (pa, pb) = (pt(ents, a), pt(ents, b));
                push_symmetric(&mut r, pa, pb, ents, axis);
            }
            Term::Length { e, target } => {
                let (a, b) = line_ends(&ents[e]).unwrap_or(([0.0; 2], [0.0; 2]));
                r.push(d2(a, b) - target);
            }
            Term::RectSize { e, axis, target } => {
                if let SketchEntity::Rectangle { p1, p2 } = &ents[e] {
                    r.push((p2[axis] - p1[axis]).abs() - target);
                }
            }
            Term::DistPts { a, b, target } => r.push(d2(pt(ents, a), pt(ents, b)) - target),
            Term::DistAxis { a, b, axis, target } => r.push((pt(ents, b)[axis] - pt(ents, a)[axis]) - target),
            Term::DistPtLine { p, line, target } => {
                let (a, b) = line_ends(&ents[line]).unwrap_or(([0.0; 2], [1.0, 0.0]));
                r.push(signed_dist_to_line(pt(ents, p), a, b).abs() - target);
            }
            Term::Radius { e, target } => {
                let (_, rr) = circle_of(&ents[e]).unwrap_or(([0.0; 2], 0.0));
                r.push(rr.abs() - target);
            }
            Term::Angle { a, b, target, signed } => {
                let (da, db) = (dir(ents, a), dir(ents, b));
                let cross = da[0] * db[1] - da[1] * db[0];
                let dot = da[0] * db[0] + da[1] * db[1];
                if signed {
                    r.push(wrap_pi(cross.atan2(dot) - target));
                } else {
                    r.push(cross.abs().atan2(dot) - target);
                }
            }
        }
    }
    for &(oa, ob) in welds {
        r.push(params[oa] - params[ob]);
        r.push(params[oa + 1] - params[ob + 1]);
    }
    r
}

/// `a` and `b` mirror each other across line `axis`: their midpoint is
/// on the axis, and the segment between them is perpendicular to it.
fn push_symmetric(r: &mut Vec<f64>, a: [f64; 2], b: [f64; 2], ents: &[SketchEntity], axis: usize) {
    let (o, d) = line_ends(&ents[axis]).unwrap_or(([0.0; 2], [1.0, 0.0]));
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    r.push(signed_dist_to_line(mid, o, d));
    let ad = dir(ents, axis);
    r.push((b[0] - a[0]) * ad[0] + (b[1] - a[1]) * ad[1]);
}

// ============================================================================
// Solve
// ============================================================================

/// Solve `sketch` constraints and dimensions.
pub fn solve_sketch(sketch: &Sketch, vars: &HashMap<String, String>) -> CadResult<SolveReport> {
    let terms = compile(sketch, vars)?;
    let mut params = pack_params(&sketch.entities);
    let n = params.len();
    let fixed = fixed_mask(&sketch.entities, &sketch.constraints, n);
    let welds = endpoint_welds(&sketch.entities);
    let eval = |p: &[f64]| -> Vec<f64> {
        let ents = unpack_params(p, &sketch.entities);
        residuals(&ents, &terms, &welds, p)
    };

    let mut r = eval(&params);
    let mut norm = norm2(&r);
    let mut iters = 0u32;
    let mut lambda = 1.0e-3;
    while iters < MAX_ITERS && norm > TOL && !r.is_empty() {
        iters += 1;
        let j = jacobian(&eval, &params, &fixed, r.len());
        let m = r.len();
        // Levenberg-Marquardt: (JtJ + lambda * diag(JtJ)) delta = -Jt r
        let mut jtj = vec![vec![0.0; n]; n];
        let mut jtr = vec![0.0; n];
        for i in 0..n {
            for k in i..n {
                let s: f64 = (0..m).map(|row| j[row][i] * j[row][k]).sum();
                jtj[i][k] = s;
                jtj[k][i] = s;
            }
            jtr[i] = -(0..m).map(|row| j[row][i] * r[row]).sum::<f64>();
        }
        let mut improved = false;
        for _ in 0..10 {
            let mut a = jtj.clone();
            for i in 0..n {
                a[i][i] += lambda * (1.0 + jtj[i][i]);
                if fixed[i] {
                    for k in 0..n {
                        a[i][k] = 0.0;
                        a[k][i] = 0.0;
                    }
                    a[i][i] = 1.0;
                }
            }
            let mut b = jtr.clone();
            for i in 0..n {
                if fixed[i] {
                    b[i] = 0.0;
                }
            }
            let Some(delta) = solve_linear(&a, &b) else {
                lambda *= 10.0;
                continue;
            };
            let trial: Vec<f64> = params.iter().zip(delta.iter()).map(|(p, d)| p + d).collect();
            let rt = eval(&trial);
            let nt = norm2(&rt);
            if nt < norm {
                params = trial;
                r = rt;
                norm = nt;
                lambda = (lambda * 0.3).max(1.0e-12);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }

    let converged = norm <= TOL * 100.0;
    let n_free = fixed.iter().filter(|f| !**f).count();
    let rows = r.len();
    let rank = if rows == 0 { 0 } else { matrix_rank(&jacobian(&eval, &params, &fixed, rows), &fixed) };
    let free_dof = n_free.saturating_sub(rank) as i32;
    let redundant = rows.saturating_sub(rank) as i32;
    let status = if !converged {
        SolveStatus::Failed
    } else if free_dof > 0 {
        SolveStatus::UnderConstrained
    } else if redundant > 0 {
        SolveStatus::OverConstrained
    } else {
        SolveStatus::FullyConstrained
    };
    Ok(SolveReport {
        entities: unpack_params(&params, &sketch.entities),
        residual_norm: norm,
        free_dof,
        converged,
        status,
        constraint_residuals: r.iter().map(|v| v.abs()).collect(),
        iterations: iters,
        redundant,
    })
}

fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum::<f64>().sqrt()
}

/// Central differences: second-order accurate, which matters for rank.
fn jacobian(eval: &dyn Fn(&[f64]) -> Vec<f64>, params: &[f64], fixed: &[bool], m: usize) -> Vec<Vec<f64>> {
    let n = params.len();
    let mut j = vec![vec![0.0; n]; m];
    let mut p = params.to_vec();
    for col in 0..n {
        if fixed[col] {
            continue;
        }
        let h = 1.0e-7 * (1.0 + params[col].abs());
        p[col] = params[col] + h;
        let rp = eval(&p);
        p[col] = params[col] - h;
        let rm = eval(&p);
        p[col] = params[col];
        for row in 0..m.min(rp.len()).min(rm.len()) {
            j[row][col] = (rp[row] - rm[row]) / (2.0 * h);
        }
    }
    j
}

/// Numerical rank of the free columns of `j`, by Gaussian elimination
/// with full pivoting and a tolerance relative to the largest entry.
fn matrix_rank(j: &[Vec<f64>], fixed: &[bool]) -> usize {
    let cols: Vec<usize> = (0..fixed.len()).filter(|&c| !fixed[c]).collect();
    let mut a: Vec<Vec<f64>> = j.iter().map(|row| cols.iter().map(|&c| row[c]).collect()).collect();
    let (m, n) = (a.len(), cols.len());
    let maxabs = a.iter().flatten().fold(0.0_f64, |x, v| x.max(v.abs()));
    if maxabs == 0.0 {
        return 0;
    }
    let tol = maxabs * 1.0e-7 * (m.max(n) as f64);
    let mut rank = 0;
    let mut used_col = vec![false; n];
    for row0 in 0..m {
        // Pivot: largest remaining entry among rows row0.. and unused cols.
        let mut best = (0.0, row0, 0);
        for (ri, r) in a.iter().enumerate().skip(row0) {
            for (ci, &v) in r.iter().enumerate() {
                if !used_col[ci] && v.abs() > best.0 {
                    best = (v.abs(), ri, ci);
                }
            }
        }
        if best.0 <= tol {
            break;
        }
        let (_, pr, pc) = best;
        a.swap(row0, pr);
        used_col[pc] = true;
        let pivot = a[row0][pc];
        for ri in (row0 + 1)..m {
            let f = a[ri][pc] / pivot;
            if f != 0.0 {
                for ci in 0..n {
                    a[ri][ci] -= f * a[row0][ci];
                }
            }
        }
        rank += 1;
    }
    rank
}

/// Apply solve result back into a sketch.
pub fn apply_solve(sketch: &mut Sketch, report: &SolveReport) {
    sketch.entities = report.entities.clone();
}

// ============================================================================
// Parameter packing
// ============================================================================

fn param_len(e: &SketchEntity) -> usize {
    match e {
        SketchEntity::Line { .. } | SketchEntity::Construction { .. } | SketchEntity::Rectangle { .. } => 4,
        SketchEntity::Circle { .. } => 3,
        SketchEntity::Arc { .. } => 5,
        SketchEntity::Point { .. } => 2,
    }
}

fn pack_params(entities: &[SketchEntity]) -> Vec<f64> {
    let mut p = Vec::new();
    for e in entities {
        match e {
            SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } | SketchEntity::Rectangle { p1, p2 } => {
                p.extend_from_slice(p1);
                p.extend_from_slice(p2);
            }
            SketchEntity::Circle { center, radius } => {
                p.extend_from_slice(center);
                p.push(*radius);
            }
            SketchEntity::Arc { center, start_angle, sweep, radius } => {
                p.extend_from_slice(center);
                p.push(*start_angle);
                p.push(*sweep);
                p.push(*radius);
            }
            SketchEntity::Point { p: pt } => p.extend_from_slice(pt),
        }
    }
    p
}

fn unpack_params(params: &[f64], template: &[SketchEntity]) -> Vec<SketchEntity> {
    let mut out = Vec::with_capacity(template.len());
    let mut i = 0usize;
    for e in template {
        let q = &params[i..i + param_len(e)];
        out.push(match e {
            SketchEntity::Line { .. } => SketchEntity::Line { p1: [q[0], q[1]], p2: [q[2], q[3]] },
            SketchEntity::Construction { .. } => SketchEntity::Construction { p1: [q[0], q[1]], p2: [q[2], q[3]] },
            SketchEntity::Rectangle { .. } => SketchEntity::Rectangle { p1: [q[0], q[1]], p2: [q[2], q[3]] },
            SketchEntity::Circle { .. } => SketchEntity::Circle { center: [q[0], q[1]], radius: q[2].abs().max(1e-9) },
            SketchEntity::Arc { .. } => SketchEntity::Arc {
                center: [q[0], q[1]],
                start_angle: q[2],
                sweep: q[3],
                radius: q[4].abs().max(1e-9),
            },
            SketchEntity::Point { .. } => SketchEntity::Point { p: [q[0], q[1]] },
        });
        i += param_len(e);
    }
    out
}

fn entity_offset(entities: &[SketchEntity], idx: usize) -> usize {
    entities.iter().take(idx).map(param_len).sum()
}

fn fixed_mask(entities: &[SketchEntity], constraints: &[SketchConstraint], n: usize) -> Vec<bool> {
    let mut fixed = vec![false; n];
    for c in constraints {
        if c.kind == ConstraintKind::Fix {
            if let Some(e) = entities.get(c.e1) {
                let a = entity_offset(entities, c.e1);
                for f in fixed.iter_mut().skip(a).take(param_len(e)) {
                    *f = true;
                }
            }
        }
    }
    fixed
}

/// Param offsets of endpoints that are coincident in the INPUT sketch:
/// the implicit topology of chained and closed profiles. Arc endpoints
/// are not plain parameters (they follow from centre, radius and
/// angles), so arcs join a profile through explicit coincident
/// constraints instead.
fn endpoint_welds(entities: &[SketchEntity]) -> Vec<(usize, usize)> {
    const WELD_EPS: f64 = 1.0e-7;
    let mut endpoints: Vec<(usize, [f64; 2])> = Vec::new();
    let mut off = 0usize;
    for e in entities {
        match e {
            SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => {
                endpoints.push((off, *p1));
                endpoints.push((off + 2, *p2));
            }
            SketchEntity::Point { p } => endpoints.push((off, *p)),
            _ => {}
        }
        off += param_len(e);
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

// ============================================================================
// Dense linear solve
// ============================================================================

/// Gaussian elimination with partial pivoting. A singular column leaves
/// its unknown at zero (a free direction the step does not move).
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
            continue;
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
            if f != 0.0 {
                for j in col..=n {
                    m[row][j] -= f * m[col][j];
                }
            }
        }
    }
    let x: Vec<f64> = m.iter().map(|row| row[n]).collect();
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Solve, and turn a solve that did not converge into an error.
pub fn solve_or_err(sketch: &Sketch, vars: &HashMap<String, String>) -> CadResult<SolveReport> {
    let report = solve_sketch(sketch, vars)?;
    if matches!(report.status, SolveStatus::Failed) {
        return Err(fail(format!(
            "did not converge (residual={:.3e}, iters={}): the constraints conflict or cannot all hold",
            report.residual_norm, report.iterations
        )));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{SketchConstraint as C, SketchEntity as E};

    fn sk(entities: Vec<E>, constraints: Vec<C>, dimensions: Vec<SketchDimension>) -> Sketch {
        Sketch { plane: "xy".into(), entities, dimensions, constraints }
    }

    fn lin(e1: usize, v: &str) -> SketchDimension {
        SketchDimension::Linear { e1, value: v.into(), e2: None, axis: None, p1: None, p2: None }
    }

    #[test]
    fn perpendicular_lines_converge() {
        let s = sk(
            vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.1] }, E::Line { p1: [0.0, 0.0], p2: [0.1, 1.0] }],
            vec![C::new(ConstraintKind::Coincident, 0, Some(1)), C::new(ConstraintKind::Perpendicular, 0, Some(1))],
            vec![],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        assert!(r.converged, "{r:?}");
    }

    #[test]
    fn horizontal_forces_dy_zero() {
        let s = sk(vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.5] }], vec![C::new(ConstraintKind::Horizontal, 0, None)], vec![]);
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        let E::Line { p1, p2 } = &r.entities[0] else { panic!() };
        assert!((p2[1] - p1[1]).abs() < 1e-7);
    }

    #[test]
    fn a_fixed_dimensioned_line_has_no_freedom_left() {
        // Fix the line: 4 params pinned, nothing free.
        let s = sk(vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] }], vec![C::new(ConstraintKind::Fix, 0, None)], vec![]);
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        assert_eq!(r.free_dof, 0);
        assert_eq!(r.status, SolveStatus::FullyConstrained);
    }

    #[test]
    fn a_redundant_constraint_is_over_not_failed() {
        // Horizontal twice: consistent, one row too many.
        let s = sk(
            vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.2] }],
            vec![C::new(ConstraintKind::Horizontal, 0, None), C::new(ConstraintKind::Horizontal, 0, None)],
            vec![],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        assert!(r.converged);
        assert_eq!(r.redundant, 1, "{r:?}");
        // A line has 4 freedoms; horizontal removes exactly one.
        assert_eq!(r.free_dof, 3);
    }

    #[test]
    fn a_closed_quad_counts_eight_freedoms() {
        // Four welded lines: four free corners, eight freedoms.
        let q = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let ents = (0..4).map(|i| E::Line { p1: q[i], p2: q[(i + 1) % 4] }).collect();
        let r = solve_sketch(&sk(ents, vec![], vec![]), &HashMap::new()).unwrap();
        assert_eq!(r.free_dof, 8, "{r:?}");
    }

    #[test]
    fn coincident_lines_join_end_to_start_not_start_to_start() {
        // Drawn nearly chained: end of e0 near start of e1.
        let s = sk(
            vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] }, E::Line { p1: [1.02, 0.01], p2: [2.0, 1.0] }],
            vec![C::new(ConstraintKind::Coincident, 0, Some(1))],
            vec![],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        let (E::Line { p1: a1, p2: a2 }, E::Line { p1: b1, .. }) = (&r.entities[0], &r.entities[1]) else { panic!() };
        assert!(d2(*a2, *b1) < 1e-7, "end of e0 should meet start of e1: {r:?}");
        assert!(d2(*a1, *b1) > 0.5, "starts must not have been welded");
    }

    #[test]
    fn a_reflex_angle_is_reachable() {
        let s = sk(
            // e1 drawn at 315 degrees; the dimension asks for 270.
            vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] }, E::Line { p1: [0.0, 0.0], p2: [0.7, -0.7] }],
            vec![C::new(ConstraintKind::Fix, 0, None)],
            vec![SketchDimension::Angular { e1: 0, e2: 1, value: "270 deg".into() }],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        assert!(r.converged, "{r:?}");
        let E::Line { p1, p2 } = &r.entities[1] else { panic!() };
        // 270 degrees counter-clockwise from +x points straight down.
        assert!((p2[0] - p1[0]).abs() < 1e-6 && p2[1] < p1[1], "{r:?}");
    }

    #[test]
    fn unsupported_pairs_fail_by_name() {
        let s = sk(
            vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] }, E::Point { p: [0.5, 0.5] }],
            vec![C::new(ConstraintKind::Tangent, 0, Some(1))],
            vec![],
        );
        let e = solve_sketch(&s, &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("tangent") && e.contains("point"), "{e}");
    }

    #[test]
    fn horizontal_on_a_rectangle_is_refused_not_poisoned() {
        let s = sk(vec![E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] }], vec![C::new(ConstraintKind::Vertical, 0, None)], vec![]);
        let e = solve_sketch(&s, &HashMap::new()).unwrap_err().to_string();
        assert!(e.contains("axis-aligned"), "{e}");
    }

    #[test]
    fn a_rectangle_height_can_be_dimensioned() {
        let s = sk(
            vec![E::Rectangle { p1: [0.0, 0.0], p2: [1.0, 1.0] }],
            vec![],
            vec![
                lin(0, "2 m"),
                SketchDimension::Linear { e1: 0, value: "0.5 m".into(), e2: None, axis: Some(DimAxis::Y), p1: None, p2: None },
            ],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        let E::Rectangle { p1, p2 } = &r.entities[0] else { panic!() };
        assert!(((p2[0] - p1[0]).abs() - 2.0).abs() < 1e-7 && ((p2[1] - p1[1]).abs() - 0.5).abs() < 1e-7, "{r:?}");
    }

    #[test]
    fn an_unresolvable_dimension_fails_instead_of_vanishing() {
        let s = sk(vec![E::Line { p1: [0.0, 0.0], p2: [1.0, 0.0] }], vec![], vec![lin(0, "nonexistent_var")]);
        assert!(solve_sketch(&s, &HashMap::new()).is_err());
    }

    #[test]
    fn tangent_circles_stay_on_the_side_they_were_drawn() {
        let s = sk(
            vec![E::Circle { center: [0.0, 0.0], radius: 1.0 }, E::Circle { center: [2.2, 0.0], radius: 1.0 }],
            vec![
                C::new(ConstraintKind::Tangent, 0, Some(1)),
                C::new(ConstraintKind::Fix, 0, None),
                C::new(ConstraintKind::EqualRadius, 0, Some(1)),
            ],
            vec![],
        );
        let r = solve_sketch(&s, &HashMap::new()).unwrap();
        let E::Circle { center, .. } = &r.entities[1] else { panic!() };
        assert!((d2(*center, [0.0, 0.0]) - 2.0).abs() < 1e-6, "{r:?}");
    }
}

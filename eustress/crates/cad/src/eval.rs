//! Deterministic feature-tree evaluator.
//!
//! Walks a [`FeatureTree`] in declaration order and produces bodies: truck
//! solids, each with a stable name for every face (see [`crate::topology`]).
//!
//! ## What a part is
//!
//! A part is a list of bodies, not one solid. The single running body this
//! replaces could not represent the result of two things that do not touch:
//! mirror a bracket to the other side, or pattern a boss along a rail, and
//! the union of disjoint solids has no intersection curves, so truck returns
//! nothing and the old evaluator kept whichever operand it had last. Now:
//!
//! - `new_body` adds a body.
//! - `add` joins every body it touches into one (the oldest keeps its name);
//!   if it touches none it becomes a body of its own.
//! - `subtract` cuts every body it touches, and fails loudly if it touches
//!   none.
//! - `intersect` keeps only what each body shares with the feature.
//!
//! A feature can be limited to named bodies with its `bodies` field.
//!
//! ## Where a sketch is
//!
//! Every sketch is placed by a [`Frame`] resolved from `Sketch.plane`: a
//! built-in plane, a reference plane feature, or a planar face by name.
//! Extrude runs along the frame normal, a hole drills along it, a revolve
//! happens in its plane.
//!
//! ## Honesty rules
//!
//! A feature either does what its fields say, or it fails with a reason. A
//! result that is geometry but not the geometry asked for (a boolean that
//! could not be computed and was skipped, a pattern instance that would not
//! combine) is reported as `degraded`, because a caller that only checks
//! `ok` would otherwise build on a wrong body without knowing it.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use truck_modeling::*;

use crate::build;
use crate::frame::{reflection_about, Frame};
use crate::profile::{profile_of, Region};
use crate::topology::{self, base_name, edges_of, faces_of, inherit_names, Body, EdgeInfo, FaceInfo, Named, Prism, Replay};
use crate::{CadError, CadResult, Feature, FeatureEntry, FeatureOp, FeatureTree, Sketch, SketchEntity};

/// Output of a tree evaluation.
pub struct EvalOutput {
    /// Every body in the part, in creation order.
    pub bodies: Vec<Body>,
    /// The first body's solid. For a single-body part, the part.
    pub body: Option<Solid>,
    /// All bodies tessellated into one mesh, with per-triangle face ids.
    pub mesh: Option<EvalMesh>,
    pub entry_status: Vec<EntryStatus>,
    /// Reference planes defined by the tree, by feature name.
    pub planes: HashMap<String, Frame>,
}

/// Flat triangle arrays: the engine lifts these into a Bevy `Mesh` and the
/// glTF exporter writes them as a primitive.
///
/// Every triangle records the face it belongs to (`face_ids` indexes
/// `face_names` and `face_bodies`), which is what lets the Studio highlight
/// and pick a face or an edge by the same name a feature uses.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EvalMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Per triangle: index into `face_names` and `face_bodies`.
    #[serde(default)]
    pub face_ids: Vec<u32>,
    #[serde(default)]
    pub face_names: Vec<String>,
    #[serde(default)]
    pub face_bodies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EntryStatus {
    pub name: String,
    pub ok: bool,
    pub message: String,
    /// The feature produced geometry, but not the geometry it was asked
    /// for: a boolean failed and was skipped, or only some pattern
    /// instances combined. Distinct from `ok: false`, and more dangerous,
    /// because everything downstream keeps working on a wrong body.
    pub degraded: bool,
}

// ============================================================================
// Booleans
// ============================================================================

/// Tolerance ladder for normalized boolean ops, relative to geometry
/// normalized so the geometric mean of the operands' diagonals is 1. The
/// shapeops 0.4 success landscape is jagged (0.005 can succeed where 0.01
/// and 0.003 both fail), so the ladder is dense; failed rungs return fast.
const BOOLEAN_TOLERANCE_LADDER: [f64; 6] = [0.005, 0.01, 0.002, 0.02, 0.05, 0.001];

/// Run a truck-shapeops binary op with **scale normalization**.
///
/// shapeops 0.4 has an absolute scale floor: geometry that booleans fine at
/// unit scale returns `None` at centimetre scale (`tests/shapeops_probe.rs`).
/// The engine is metre-native, so real parts sit under the floor. Both
/// operands are scaled toward unit size (geometric mean of their diagonals,
/// clamped so a huge construction slab cannot push the small operand off the
/// reliable band), the op runs, and the result is scaled back.
///
/// The result's `IntersectionCurve` edges carry the composed transform, so
/// anything that evaluates them must re-normalize first (booleans do, and
/// `tessellate_solid` does). `builder::transformed` only composes matrices
/// and is safe.
fn boolean_normalized<F>(a: &Solid, b: &Solid, op: F) -> Option<Solid>
where
    F: Fn(&Solid, &Solid, f64) -> Option<Solid>,
{
    let da = solid_bbox_diagonal(a);
    let db = solid_bbox_diagonal(b);
    let mean = (da * db).sqrt();
    let scale = if mean > 1.0e-12 {
        let s = 1.0 / mean;
        let d_min = da.min(db);
        s.clamp(0.05 / d_min.max(1.0e-12), 20.0 / d_min.max(1.0e-12))
    } else {
        1.0
    };
    let a = builder::transformed(a, Matrix4::from_scale(scale));
    let b = builder::transformed(b, Matrix4::from_scale(scale));
    for tol in BOOLEAN_TOLERANCE_LADDER {
        // truck-geometry unwraps Newton projections internally, so a
        // tolerance its numerics cannot handle PANICS rather than returning
        // None. Treat a panic as "this rung failed".
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(&a, &b, tol)));
        if let Ok(Some(out)) = result {
            return Some(builder::transformed(&out, Matrix4::from_scale(1.0 / scale)));
        }
    }
    None
}

fn solid_bbox_diagonal(s: &Solid) -> f64 {
    match topology::bounds(s) {
        Some((lo, hi)) => ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt(),
        None => 1.0,
    }
}

/// Union. `pub(crate)` so `parts_csg` reuses the normalized path.
pub(crate) fn boolean_or(a: &Solid, b: &Solid) -> Option<Solid> {
    boolean_normalized(a, b, |x, y, tol| truck_shapeops::or(x, y, tol))
}

pub(crate) fn boolean_and(a: &Solid, b: &Solid) -> Option<Solid> {
    boolean_normalized(a, b, |x, y, tol| truck_shapeops::and(x, y, tol))
}

/// Difference `A \ B = A ∩ ¬B`. shapeops 0.4 exports only `or` and `and`;
/// `Solid::not` turns a solid inside out, which is how truck's own
/// punched-cube example subtracts. Invert AFTER the rescale: scaling an
/// already-inverted solid breaks shapeops.
///
/// Coplanar or flush faces between operands degenerate the intersection
/// curve and the op returns `None`, so every cutter in this crate protrudes
/// through the faces it enters.
pub(crate) fn boolean_not(a: &Solid, b: &Solid) -> Option<Solid> {
    boolean_normalized(a, b, |x, y, tol| {
        let mut y_inverted = y.clone();
        y_inverted.not();
        truck_shapeops::and(x, &y_inverted, tol)
    })
}

// ============================================================================
// The model under construction
// ============================================================================

/// A feature's own geometry: the prism an extrude built, the cutter a hole
/// drilled with. Pattern and Mirror replicate these, not the whole part.
#[derive(Clone)]
struct Gen {
    solid: Solid,
    names: Vec<String>,
    op: FeatureOp,
    prism: Option<Prism>,
}

#[derive(Default)]
struct Outcome {
    notes: Vec<String>,
    degraded: Vec<String>,
}

impl Outcome {
    fn note(&mut self, s: impl Into<String>) {
        self.notes.push(s.into());
    }
    fn degrade(&mut self, s: impl Into<String>) {
        self.degraded.push(s.into());
    }
    fn absorb(&mut self, o: Outcome) {
        self.notes.extend(o.notes);
        self.degraded.extend(o.degraded);
    }
    fn message(&self) -> String {
        let mut parts: Vec<String> = self.notes.clone();
        parts.extend(self.degraded.iter().map(|d| format!("DEGRADED: {d}")));
        if parts.is_empty() {
            "ok".to_string()
        } else {
            parts.join("; ")
        }
    }
}

struct Model<'t> {
    vars: &'t HashMap<String, String>,
    bodies: Vec<Body>,
    planes: HashMap<String, Frame>,
    sketches: HashMap<String, (Sketch, Frame)>,
    sketch_errors: HashMap<String, String>,
    generated: HashMap<String, Vec<Gen>>,
    seq: u64,
    /// Largest radius asked of a legacy positional fillet/chamfer: applied
    /// as the old visual-only crease soften after tessellation.
    legacy_round: f64,
}

fn err(feature: &str, reason: impl Into<String>) -> CadError {
    CadError::EvalFailed { feature: feature.to_string(), reason: reason.into() }
}

/// Walk the tree top to bottom.
pub fn evaluate_tree(tree: &FeatureTree) -> CadResult<EvalOutput> {
    let mut m = Model {
        vars: &tree.variables,
        bodies: Vec::new(),
        planes: HashMap::new(),
        sketches: HashMap::new(),
        sketch_errors: HashMap::new(),
        generated: HashMap::new(),
        seq: 0,
        legacy_round: 0.0,
    };
    let mut entry_status = Vec::with_capacity(tree.entries.len());

    for entry in &tree.entries {
        if entry.is_suppressed() {
            entry_status.push(EntryStatus {
                name: entry.name().to_string(),
                ok: true,
                message: "(suppressed)".to_string(),
                degraded: false,
            });
            continue;
        }
        match entry {
            FeatureEntry::Sketch { name, body } => entry_status.push(m.add_sketch(name, body)),
            FeatureEntry::Feature { name, body } => {
                m.seq += 1;
                // A feature that fails leaves the part exactly as it was:
                // half-applied patterns and partial cuts are worse than
                // nothing, because they look like the requested result.
                let snapshot = m.bodies.clone();
                match m.feature(name, body) {
                    Ok(o) => entry_status.push(EntryStatus {
                        name: name.clone(),
                        ok: true,
                        message: o.message(),
                        degraded: !o.degraded.is_empty(),
                    }),
                    Err(e) => {
                        m.bodies = snapshot;
                        entry_status.push(EntryStatus {
                            name: name.clone(),
                            ok: false,
                            message: e.to_string(),
                            degraded: false,
                        });
                    }
                }
            }
            FeatureEntry::Suppressed { .. } => unreachable!(),
        }
    }

    let tolerance = tree.metadata.mesh_tolerance.unwrap_or(DEFAULT_MESH_TOLERANCE);
    let mut mesh = if m.bodies.is_empty() {
        None
    } else {
        let meshes: Vec<EvalMesh> = m.bodies.iter().map(|b| tessellate_body(b, tolerance)).collect();
        Some(merge_meshes(meshes))
    };
    if m.legacy_round > 1.0e-9 {
        if let Some(ref mut mm) = mesh {
            soften_mesh_creases(mm, m.legacy_round as f32);
        }
    }
    let body = m.bodies.first().map(|b| b.solid.clone());
    Ok(EvalOutput { bodies: m.bodies, body, mesh, entry_status, planes: m.planes })
}

impl<'t> Model<'t> {
    // ── Lookups ─────────────────────────────────────────────────────

    fn len(&self, s: &str) -> CadResult<f64> {
        resolve_length_meters(s, self.vars)
    }

    fn angle(&self, s: &str) -> CadResult<f64> {
        resolve_angle_radians(s, self.vars)
    }

    /// Characteristic size of the model, for scale-aware tolerances.
    fn scale(&self) -> f64 {
        self.bodies.iter().map(|b| solid_bbox_diagonal(&b.solid)).fold(0.0_f64, f64::max).max(1.0e-3)
    }

    /// Surface-identity tolerance for lineage. Operand surfaces come back
    /// from the scale-normalized booleans off by a few ulps, never by a
    /// nanometre.
    fn lineage_tol(&self) -> f64 {
        1.0e-9 * (1.0 + self.scale())
    }

    fn add_sketch(&mut self, name: &str, sk: &Sketch) -> EntryStatus {
        let (solved, mut message, mut ok) = if sk.constraints.is_empty() && sk.dimensions.is_empty() {
            (sk.clone(), "sketch loaded".to_string(), true)
        } else {
            match crate::solver::solve_sketch(sk, self.vars) {
                Ok(report) => {
                    let mut s2 = sk.clone();
                    crate::solver::apply_solve(&mut s2, &report);
                    let msg = format!(
                        "solved {:?} residual={:.2e} dof={} iters={}",
                        report.status, report.residual_norm, report.free_dof, report.iterations
                    );
                    (s2, msg, report.converged || report.residual_norm < 1e-3)
                }
                Err(e) => (sk.clone(), format!("solver: {e}"), false),
            }
        };
        match self.resolve_frame(&sk.plane) {
            Ok(frame) => {
                self.sketches.insert(name.to_string(), (solved, frame));
                self.sketch_errors.remove(name);
            }
            Err(e) => {
                ok = false;
                message = format!("plane '{}' does not resolve: {e}", sk.plane);
                self.sketch_errors.insert(name.to_string(), message.clone());
            }
        }
        EntryStatus { name: name.to_string(), ok, message, degraded: false }
    }

    fn sketch(&self, name: &str) -> CadResult<(Sketch, Frame)> {
        if let Some((s, f)) = self.sketches.get(name) {
            return Ok((s.clone(), *f));
        }
        if let Some(e) = self.sketch_errors.get(name) {
            return Err(err("sketch", format!("sketch '{name}' could not be placed: {e}")));
        }
        Err(CadError::SketchNotFound(name.to_string()))
    }

    fn all_faces(&self) -> Vec<(usize, FaceInfo)> {
        self.bodies.iter().enumerate().flat_map(|(i, b)| faces_of(b).into_iter().map(move |f| (i, f))).collect()
    }

    fn find_face(&self, name: &str) -> Option<(usize, FaceInfo)> {
        let q = name.trim();
        let all = self.all_faces();
        if let Some(hit) = all.iter().find(|(_, f)| f.name == q) {
            return Some(hit.clone());
        }
        // `Body/Face` form, for parts where two bodies share face names.
        if let Some((body, face)) = q.split_once('/') {
            if let Some(hit) = all.iter().find(|(i, f)| self.bodies[*i].name == body && f.name == face) {
                return Some(hit.clone());
            }
        }
        None
    }

    fn face_names_hint(&self) -> String {
        let mut names: Vec<String> = self.all_faces().into_iter().filter(|(_, f)| f.plane.is_some()).map(|(_, f)| f.name).collect();
        names.sort();
        names.truncate(12);
        names.join(", ")
    }

    /// Resolve a plane reference: built-in, reference plane, or planar face.
    fn resolve_frame(&self, name: &str) -> CadResult<Frame> {
        let q = name.trim();
        if let Some(f) = Frame::builtin(q) {
            return Ok(f);
        }
        if let Some(f) = self.planes.get(q) {
            return Ok(*f);
        }
        if let Some((_, face)) = self.find_face(q) {
            let Some(pf) = face.plane else {
                return Err(err("plane", format!("face '{q}' is {}, not planar, so it cannot carry a sketch or act as a plane", face.kind)));
            };
            // Anchor the frame on the world origin projected into the face
            // plane, with axes from the world axes: sketch coordinates on a
            // face then read like world coordinates, and they do not move
            // when an upstream edit reshapes the face.
            let o = Point3::origin();
            let foot = o - pf.z * (o - pf.origin).dot(pf.z);
            return Frame::from_origin_normal(foot, pf.z).ok_or_else(|| err("plane", "face normal is degenerate"));
        }
        let mut planes: Vec<&String> = self.planes.keys().collect();
        planes.sort();
        Err(err(
            "plane",
            format!(
                "unknown plane '{q}'. Built-in: xy, xz, yz. Reference planes: [{}]. Planar faces include: [{}]",
                planes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                self.face_names_hint()
            ),
        ))
    }

    fn find_edge(&self, name: &str) -> CadResult<(usize, EdgeInfo)> {
        for (i, b) in self.bodies.iter().enumerate() {
            let edges = edges_of(b);
            if let Some(e) = topology::find_edge(&edges, name) {
                return Ok((i, e.clone()));
            }
        }
        Err(err(
            "edge",
            format!(
                "no edge named '{name}'. Edges are named by the two faces they separate, \
                 'FaceA | FaceB'; list them with cad_list_topology"
            ),
        ))
    }

    /// Resolve an axis: a world axis, a line in the given sketch (`e3`), or
    /// a straight edge by name.
    fn resolve_axis(&self, s: &str, sketch: Option<(&Sketch, &Frame)>) -> CadResult<(Point3, Vector3)> {
        let q = s.trim();
        match q.to_ascii_lowercase().as_str() {
            "x" | "world/x" => return Ok((Point3::origin(), Vector3::unit_x())),
            "y" | "world/y" => return Ok((Point3::origin(), Vector3::unit_y())),
            "z" | "world/z" => return Ok((Point3::origin(), Vector3::unit_z())),
            _ => {}
        }
        if let Some((sk, frame)) = sketch {
            let tail = q.rsplit('.').next().unwrap_or(q);
            if let Some(idx) = tail.strip_prefix('e').and_then(|n| n.parse::<usize>().ok()) {
                return match sk.entities.get(idx) {
                    Some(SketchEntity::Line { p1, p2 }) | Some(SketchEntity::Construction { p1, p2 }) => {
                        let a = frame.to_world(*p1);
                        let b = frame.to_world(*p2);
                        let d = b - a;
                        if d.magnitude() < 1.0e-12 {
                            Err(err("axis", format!("sketch line e{idx} has zero length")))
                        } else {
                            Ok((a, d.normalize()))
                        }
                    }
                    _ => Err(err("axis", format!("sketch entity e{idx} is not a line or construction line"))),
                };
            }
        }
        if q.contains('|') {
            let (_, e) = self.find_edge(q)?;
            if !e.straight {
                return Err(err("axis", format!("edge '{q}' is not straight")));
            }
            let a = Point3::new(e.a[0], e.a[1], e.a[2]);
            let b = Point3::new(e.b[0], e.b[1], e.b[2]);
            return Ok((a, (b - a).normalize()));
        }
        Err(err(
            "axis",
            format!("unknown axis '{q}': expected x, y or z, a sketch line such as 'e3', or a straight edge 'FaceA | FaceB'"),
        ))
    }

    fn unique_body_name(&self, base: &str) -> String {
        if !self.bodies.iter().any(|b| b.name == base) {
            return base.to_string();
        }
        (2..).map(|k| format!("{base}#{k}")).find(|n| !self.bodies.iter().any(|b| &b.name == n)).unwrap()
    }

    fn targeted(&self, i: usize, targets: &[String]) -> bool {
        targets.is_empty() || targets.iter().any(|t| t == &self.bodies[i].name)
    }

    /// Extent of the (targeted) bodies along an axis, relative to `origin`.
    fn extent_along(&self, origin: Point3, axis: Vector3, targets: &[String]) -> Option<(f64, f64)> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in 0..self.bodies.len() {
            if !self.targeted(i, targets) {
                continue;
            }
            if let Some((a, b)) = topology::extent_along(&self.bodies[i].solid, origin, axis) {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        (lo <= hi).then_some((lo, hi))
    }

    /// Do two solids share no volume? Decides whether a boolean that
    /// returned nothing failed, or simply had nothing to do.
    fn disjoint(&self, a: &Solid, b: &Solid) -> bool {
        let (Some(ba), Some(bb)) = (topology::bounds(a), topology::bounds(b)) else {
            return true;
        };
        let margin = 1.0e-9 * (1.0 + self.scale());
        if !topology::bounds_overlap(&ba, &bb, margin) {
            return true;
        }
        let coarse = 0.01 * solid_bbox_diagonal(a).max(solid_bbox_diagonal(b));
        let ma = tessellate_solid(a, coarse);
        let mb = tessellate_solid(b, coarse);
        if ma.indices.is_empty() || mb.indices.is_empty() {
            return false;
        }
        let (d, _) = crate::measure::min_distance(&ma, &mb);
        if !(d > margin) {
            return false;
        }
        // Surfaces apart, but one could still sit inside the other.
        let inside = |m: &EvalMesh, other: &EvalMesh| {
            other.positions.first().map_or(false, |p| {
                crate::measure::contains_point(m, [p[0] as f64, p[1] as f64, p[2] as f64])
            })
        };
        !(inside(&ma, &mb) || inside(&mb, &ma))
    }

    fn gens_of(&self, feature: &str, names: &[String]) -> CadResult<Vec<Gen>> {
        let mut out = Vec::new();
        for n in names {
            match self.generated.get(n) {
                Some(g) => out.extend(g.iter().cloned()),
                None => {
                    let mut known: Vec<&str> = self.generated.keys().map(|s| s.as_str()).collect();
                    known.sort_unstable();
                    return Err(err(
                        feature,
                        format!(
                            "feature '{n}' has no geometry to replicate. Features that produced geometry so far: \
                             [{}]. A referenced feature must appear EARLIER in the tree, and must build or \
                             cut geometry (a fillet or a shell modifies a body in place and has none of its own).",
                            known.join(", ")
                        ),
                    ));
                }
            }
        }
        Ok(out)
    }

    fn bodies_as_gens(&self) -> Vec<Gen> {
        self.bodies
            .iter()
            .map(|b| Gen { solid: b.solid.clone(), names: b.face_names.clone(), op: FeatureOp::Add, prism: None })
            .collect()
    }

    // ── Combining ───────────────────────────────────────────────────

    fn apply(&mut self, feature: &str, gens: Vec<Gen>, targets: &[String]) -> CadResult<Outcome> {
        let mut out = Outcome::default();
        let mut applied = 0usize;
        for g in &gens {
            let o = match g.op {
                FeatureOp::NewBody => {
                    self.push_body(feature, g);
                    Ok(Outcome::default())
                }
                FeatureOp::Add => self.join(feature, g, targets),
                FeatureOp::Subtract => self.cut(feature, g, targets),
                FeatureOp::Intersect => self.intersect(feature, g, targets),
            }?;
            if o.degraded.is_empty() {
                applied += 1;
            }
            out.absorb(o);
        }
        if gens.len() > 1 && applied < gens.len() {
            out.note(format!("{applied} of {} pieces combined", gens.len()));
        }
        self.generated.insert(feature.to_string(), gens);
        Ok(out)
    }

    fn push_body(&mut self, feature: &str, g: &Gen) {
        let name = self.unique_body_name(feature);
        let mut body = Body::fresh(name, g.solid.clone(), g.names.clone(), self.seq);
        body.prism = g.prism.clone();
        self.bodies.push(body);
    }

    fn join(&mut self, feature: &str, g: &Gen, targets: &[String]) -> CadResult<Outcome> {
        let mut out = Outcome::default();
        let tol = self.lineage_tol();
        let gb = topology::bounds(&g.solid);
        let seq = self.seq;
        let g_seq = vec![seq; g.names.len()];
        // The merged result so far: starts as the feature's own solid.
        let mut merged: Option<Body> = None;
        let mut consumed: Vec<usize> = Vec::new();
        for i in 0..self.bodies.len() {
            if !self.targeted(i, targets) {
                continue;
            }
            let overlaps = match (&gb, topology::bounds(&self.bodies[i].solid)) {
                (Some(a), Some(b)) => topology::bounds_overlap(a, &b, tol),
                _ => false,
            };
            if !overlaps {
                continue;
            }
            let (tool_solid, tool_names, tool_seq) = match &merged {
                Some(mb) => (mb.solid.clone(), mb.face_names.clone(), mb.face_seq.clone()),
                None => (g.solid.clone(), g.names.clone(), g_seq.clone()),
            };
            let body = &self.bodies[i];
            match boolean_or(&body.solid, &tool_solid) {
                Some(r) => {
                    let (names, seqs) = inherit_names(
                        &r,
                        &[body.named(), Named { solid: &tool_solid, names: &tool_names, seq: &tool_seq }],
                        tol,
                        feature,
                    );
                    // The first body merged keeps its name; its prism
                    // record survives a join with the FEATURE (replayable),
                    // not a merge of two existing bodies.
                    let (name, prism) = match &merged {
                        None => {
                            let prism = body.prism.clone().map(|mut p| {
                                p.history.push(Replay { op: FeatureOp::Add, tool: g.solid.clone(), tool_names: g.names.clone(), tool_seq: seq });
                                p
                            });
                            (body.name.clone(), prism)
                        }
                        Some(mb) => (mb.name.clone(), None),
                    };
                    merged = Some(Body { name, solid: r, face_names: names, face_seq: seqs, prism });
                    consumed.push(i);
                }
                None => {
                    if !self.disjoint(&self.bodies[i].solid, &tool_solid) {
                        out.degrade(format!(
                            "could not join with body '{}': the kernel returned no union although the two touch",
                            self.bodies[i].name
                        ));
                    }
                }
            }
        }
        match merged {
            Some(mb) => {
                let at = consumed[0];
                for &i in consumed.iter().rev() {
                    self.bodies.remove(i);
                }
                self.bodies.insert(at, mb);
                if consumed.len() > 1 {
                    out.note(format!("merged {} bodies", consumed.len()));
                }
            }
            None => {
                self.push_body(feature, g);
                if self.bodies.len() > 1 {
                    out.note(format!("touches no existing body; added as body '{}'", self.bodies.last().map(|b| b.name.as_str()).unwrap_or("")));
                }
            }
        }
        Ok(out)
    }

    fn cut(&mut self, feature: &str, g: &Gen, targets: &[String]) -> CadResult<Outcome> {
        let mut out = Outcome::default();
        let tol = self.lineage_tol();
        let gb = topology::bounds(&g.solid);
        let seq = self.seq;
        let g_seq = vec![seq; g.names.len()];
        let mut touched = 0usize;
        for i in 0..self.bodies.len() {
            if !self.targeted(i, targets) {
                continue;
            }
            let overlaps = match (&gb, topology::bounds(&self.bodies[i].solid)) {
                (Some(a), Some(b)) => topology::bounds_overlap(a, &b, tol),
                _ => false,
            };
            if !overlaps {
                continue;
            }
            match boolean_not(&self.bodies[i].solid, &g.solid) {
                Some(r) => {
                    let body = &self.bodies[i];
                    let (names, seqs) = inherit_names(
                        &r,
                        &[body.named(), Named { solid: &g.solid, names: &g.names, seq: &g_seq }],
                        tol,
                        feature,
                    );
                    let body = &mut self.bodies[i];
                    body.solid = r;
                    body.face_names = names;
                    body.face_seq = seqs;
                    if let Some(p) = &mut body.prism {
                        p.history.push(Replay { op: FeatureOp::Subtract, tool: g.solid.clone(), tool_names: g.names.clone(), tool_seq: seq });
                    }
                    touched += 1;
                }
                None => {
                    if !self.disjoint(&self.bodies[i].solid, &g.solid) {
                        out.degrade(format!(
                            "could not cut body '{}': the kernel returned no result although the cutter meets it",
                            self.bodies[i].name
                        ));
                    }
                }
            }
        }
        if touched == 0 && out.degraded.is_empty() {
            return Err(err(
                feature,
                "the cut does not intersect any body, so it removes nothing; check its position and direction",
            ));
        }
        Ok(out)
    }

    fn intersect(&mut self, feature: &str, g: &Gen, targets: &[String]) -> CadResult<Outcome> {
        let mut out = Outcome::default();
        let tol = self.lineage_tol();
        let seq = self.seq;
        let g_seq = vec![seq; g.names.len()];
        let mut keep: Vec<bool> = vec![true; self.bodies.len()];
        let mut any = false;
        for i in 0..self.bodies.len() {
            if !self.targeted(i, targets) {
                continue;
            }
            match boolean_and(&self.bodies[i].solid, &g.solid) {
                Some(r) => {
                    let body = &self.bodies[i];
                    let (names, seqs) = inherit_names(
                        &r,
                        &[body.named(), Named { solid: &g.solid, names: &g.names, seq: &g_seq }],
                        tol,
                        feature,
                    );
                    let body = &mut self.bodies[i];
                    body.solid = r;
                    body.face_names = names;
                    body.face_seq = seqs;
                    if let Some(p) = &mut body.prism {
                        p.history.push(Replay { op: FeatureOp::Intersect, tool: g.solid.clone(), tool_names: g.names.clone(), tool_seq: seq });
                    }
                    any = true;
                }
                None => {
                    if self.disjoint(&self.bodies[i].solid, &g.solid) {
                        keep[i] = false;
                        out.note(format!("body '{}' shares nothing with the feature and was removed", self.bodies[i].name));
                    } else {
                        out.degrade(format!("could not intersect body '{}'", self.bodies[i].name));
                    }
                }
            }
        }
        if !any && out.degraded.is_empty() {
            return Err(err(feature, "the intersection is empty: the feature shares no volume with any body"));
        }
        let mut k = 0;
        self.bodies.retain(|_| {
            let r = keep[k];
            k += 1;
            r
        });
        Ok(out)
    }

    // ── Feature dispatch ────────────────────────────────────────────

    fn feature(&mut self, name: &str, f: &Feature) -> CadResult<Outcome> {
        use Feature::*;
        match f {
            Extrude { sketch, depth, end_condition, combine, draft_angle, both_sides, to, reverse, thin, bodies } => self
                .extrude(name, sketch, depth, *end_condition, *combine, draft_angle, *both_sides, to.as_deref(), *reverse, thin.as_deref(), bodies),
            Revolve { sketch, axis, angle, combine, both_sides, bodies } => {
                self.revolve(name, sketch, axis, angle, *combine, *both_sides, bodies)
            }
            Hole {
                sketch_point,
                diameter,
                depth,
                counterbore_diameter,
                counterbore_depth,
                countersink_diameter,
                countersink_angle,
                tap_class,
            } => self.hole(
                name,
                sketch_point,
                diameter,
                depth,
                counterbore_diameter.as_deref(),
                counterbore_depth.as_deref(),
                countersink_diameter.as_deref(),
                countersink_angle.as_deref(),
                tap_class.as_deref(),
            ),
            Mirror { plane, features, combine } => self.mirror(name, plane, features, *combine),
            Pattern { kind, features, count, spacing, direction, axis, angle, direction_ref, combine } => self.pattern(
                name,
                *kind,
                features,
                count.resolve(self.vars)?,
                spacing.as_deref(),
                *direction,
                axis.as_deref(),
                angle.as_deref(),
                direction_ref.as_deref(),
                *combine,
            ),
            Boolean { target, boolean_op, tools, keep_tools } => self.boolean(name, target, *boolean_op, tools, *keep_tools),
            Split { plane } => self.split(name, plane),
            Fillet { edges, radius, propagate_tangent } => {
                if !*propagate_tangent {
                    return Err(CadError::NotImplemented(
                        "Fillet propagate_tangent = false is not implemented: edges are filleted exactly as \
                         listed, and tangent chains are not extended automatically in either mode"
                            .into(),
                    ));
                }
                let r = self.len(radius)?;
                self.blend(name, edges, crate::blend::BlendKind::Fillet { r })
            }
            Chamfer { edges, distance, distance2, angle } => {
                let d = self.len(distance)?;
                let second = match (distance2, angle) {
                    (Some(_), Some(_)) => {
                        return Err(err(name, "give either distance2 or angle for an asymmetric chamfer, not both"))
                    }
                    (Some(d2), None) => crate::blend::Second::Distance(self.len(d2)?),
                    (None, Some(a)) => crate::blend::Second::Angle(self.angle(a)?),
                    (None, None) => crate::blend::Second::Equal,
                };
                self.blend(name, edges, crate::blend::BlendKind::Chamfer { d, second })
            }
            Shell { open_faces, wall_thickness } => {
                let t = self.len(wall_thickness)?;
                self.shell(name, open_faces, t)
            }
            Sweep { profile, path, combine } => self.sweep(name, profile, path, *combine),
            Loft { profiles, guide_curves, combine } => {
                if !guide_curves.is_empty() {
                    return Err(CadError::NotImplemented(
                        "Loft guide_curves are not implemented: the loft is ruled between consecutive \
                         sections. Remove guide_curves, or add intermediate sections to shape it."
                            .into(),
                    ));
                }
                self.loft(name, profiles, *combine)
            }
            ReferencePlane { plane } => self.reference_plane(name, plane),
        }
    }

    // ── Extrude ─────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn extrude(
        &mut self,
        name: &str,
        sketch: &str,
        depth: &str,
        end: crate::EndCondition,
        combine: FeatureOp,
        draft_angle: &str,
        both_sides: bool,
        to: Option<&str>,
        reverse: bool,
        thin: Option<&str>,
        bodies: &[String],
    ) -> CadResult<Outcome> {
        use crate::EndCondition::*;
        let (sk, frame) = self.sketch(sketch)?;
        let profile = profile_of(&sk)?;
        let mut out = Outcome::default();
        if profile.open_segments > 0 {
            out.note(format!(
                "{} open segment(s) in '{sketch}' enclose nothing and were not extruded",
                profile.open_segments
            ));
        }
        let draft = self.angle(draft_angle)?;
        let regions = match thin {
            Some(t) => build::thin_regions(&profile.regions, self.len(t)?, name)?,
            None => profile.regions.clone(),
        };

        let uses_depth = matches!(end, Blind | MidPlane);
        let raw = if uses_depth { self.len(depth)? } else { 0.0 };
        let mut dir = if reverse { -1.0 } else { 1.0 };
        let d = if raw < 0.0 {
            dir = -dir;
            -raw
        } else {
            raw
        };
        if uses_depth && !(d > 0.0) {
            return Err(err(name, "depth is zero, so the extrusion has no volume"));
        }
        // A face's frame normal points out of the material, so a cut
        // sketched on a face would, by default, remove nothing. When the
        // caller chose no direction and only the other side meets material,
        // cut into it, as Fusion and SolidWorks do and as the hole feature
        // drills.
        if matches!(end, Blind)
            && !both_sides
            && !reverse
            && raw > 0.0
            && matches!(combine, FeatureOp::Subtract | FeatureOp::Intersect)
        {
            let meshes = self.target_meshes(bodies);
            if !meets_material(&meshes, &regions, &frame, 1.0, d) && meets_material(&meshes, &regions, &frame, -1.0, d) {
                dir = -1.0;
                out.note("the cut runs against the sketch normal, into the material; the normal side is empty");
            }
        }

        // (start, length, caps reversed, trim plane)
        let (start, length, caps_reversed, trim): (f64, f64, bool, Option<Frame>) = match end {
            Blind if both_sides => (-d * 0.5, d, false, None),
            MidPlane => (-d * 0.5, d, false, None),
            Blind => {
                if dir > 0.0 {
                    (0.0, d, false, None)
                } else {
                    (-d, d, true, None)
                }
            }
            ThroughAll => {
                let Some((lo, hi)) = self.extent_along(frame.origin, frame.z, bodies) else {
                    return Err(err(
                        name,
                        "end_condition = \"through_all\" needs an existing body to pass through; this is the \
                         first body in the tree",
                    ));
                };
                // Through everything, both ways: a through cut from a sketch
                // on the top face and one from the plane under the part must
                // both clear it. Overcut so no cap is flush with a face.
                let over = ((hi - lo).abs() * 0.05).max(1.0e-4);
                let s = lo.min(0.0) - over;
                (s, (hi.max(0.0) - s) + over, false, None)
            }
            ToPlane | ToSurface => {
                let Some(t) = to else {
                    return Err(err(
                        name,
                        format!(
                            "end_condition = \"{}\" needs `to`: the plane or planar face to stop at",
                            if matches!(end, ToPlane) { "to_plane" } else { "to_surface" }
                        ),
                    ));
                };
                let target = self.resolve_frame(t)?;
                self.span_to_plane(name, &regions, &frame, &target)?
            }
            UpToNext => {
                let target = self.next_face_plane(name, &regions, &frame, dir, bodies)?;
                self.span_to_plane(name, &regions, &frame, &target)?
            }
        };

        if draft.abs() > 1.0e-12 && (both_sides || matches!(end, MidPlane)) {
            return Err(CadError::NotImplemented(
                "a draft on a two-sided extrusion is not implemented: draft one side at a time, or remove the draft"
                    .into(),
            ));
        }
        if draft.abs() >= PI * 0.5 - 1.0e-6 {
            return Err(err(name, "draft_angle must be less than 90 degrees"));
        }
        let (start, length) = if matches!(end, Blind) && !both_sides && trim.is_none() && draft.abs() <= 1.0e-12 {
            self.embed(&regions, &frame, start, length, caps_reversed, combine, bodies)
        } else {
            (start, length)
        };

        let mut gens = Vec::with_capacity(regions.len());
        for (ri, region) in regions.iter().enumerate() {
            let (mut solid, mut names) = if draft.abs() > 1.0e-12 {
                // Taper toward the far end: that is the lower end when the
                // extrusion ran against the normal.
                let shrink = -length * draft.tan();
                let far = build::offset_region(region, shrink)?;
                let sections = if caps_reversed {
                    vec![(far, frame.offset(start)), (region.clone(), frame.offset(start + length))]
                } else {
                    vec![(region.clone(), frame.offset(start)), (far, frame.offset(start + length))]
                };
                let (s, mut n) = build::loft(&sections, name)?;
                if caps_reversed {
                    let last = n.len() - 1;
                    n.swap(0, last);
                }
                if ri > 0 {
                    let first_ix = 0;
                    let last = n.len() - 1;
                    n[first_ix] = format!("{}.r{}", n[first_ix], ri + 1);
                    n[last] = format!("{}.r{}", n[last], ri + 1);
                }
                (s, n)
            } else {
                build::prism(region, &frame, start, length, name, ri, caps_reversed)?
            };
            let mut prism = None;
            if let Some(plane) = &trim {
                let extent = 10.0 * (self.scale() + length + region.outer.extent());
                // Keep the side of the target plane the sketch is on.
                let keep_frame = if plane.signed_distance(frame.origin) > 0.0 { plane.reversed() } else { *plane };
                let (slab, slab_names) = build::halfspace_slab(&keep_frame, extent, name)?;
                let slab_names: Vec<String> = slab_names
                    .into_iter()
                    .map(|n| if n == format!("{name}.cut") { format!("{name}.cap_end") } else { n })
                    .collect();
                let seq = vec![self.seq; names.len()];
                let slab_seq = vec![self.seq; slab_names.len()];
                let r = boolean_and(&solid, &slab).ok_or_else(|| {
                    err(name, "could not trim the extrusion at the target plane (the kernel returned no result)")
                })?;
                let (n2, _) = inherit_names(
                    &r,
                    &[Named { solid: &solid, names: &names, seq: &seq }, Named { solid: &slab, names: &slab_names, seq: &slab_seq }],
                    self.lineage_tol(),
                    name,
                );
                solid = r;
                names = n2;
            } else if draft.abs() <= 1.0e-12 {
                prism = Some(Prism {
                    feature: name.to_string(),
                    frame,
                    regions: vec![region.clone()],
                    region_index: ri,
                    start,
                    length,
                    reversed: caps_reversed,
                    seq: self.seq,
                    history: Vec::new(),
                });
            }
            gens.push(Gen { solid, names, op: combine, prism });
        }
        let o = self.apply(name, gens, bodies)?;
        out.absorb(o);
        Ok(out)
    }

    /// Keep a join or a cut from sharing a face's plane with the body.
    ///
    /// Extruding from a sketch on a face puts the new solid's base cap in
    /// the very plane of that face, and shapeops degenerates on coplanar
    /// operands: the join of a boss to the face it stands on, the most
    /// common move in part modeling, would fail. So a JOIN whose base sits
    /// on material is buried a little way into it, and a CUT that starts
    /// on a face is started a little way out in the air in front of it (as
    /// the hole feature has always done). A cut that ends exactly at the
    /// far face is carried through it too. The extra length merges into the
    /// body or falls in empty space, so the resulting solid is the same.
    ///
    /// Material is probed under the profile itself, not by bounding box, so
    /// an unrelated tall body elsewhere in the part cannot trigger it.
    #[allow(clippy::too_many_arguments)]
    fn embed(
        &self,
        regions: &[Region],
        frame: &Frame,
        start: f64,
        length: f64,
        reversed: bool,
        combine: FeatureOp,
        targets: &[String],
    ) -> (f64, f64) {
        let travel = if reversed { -1.0 } else { 1.0 };
        let Some(region) = regions.first() else { return (start, length) };
        let p2 = interior_point(region);
        let base = frame.to_world(p2);
        let along = frame.z * travel;
        let meshes = self.target_meshes(targets);
        let inside = |d: f64| {
            let q = base + along * d;
            meshes.iter().any(|m| crate::measure::contains_point(m, [q.x, q.y, q.z]))
        };
        let (back, front) = match combine {
            FeatureOp::Add => {
                let d = 0.25 * length;
                // Material directly behind the base, for the whole depth
                // the base would be buried.
                if inside(-0.5 * d) && inside(-d) {
                    (d, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }
            FeatureOp::Subtract | FeatureOp::Intersect => {
                let d = (0.05 * length).max(1.0e-4);
                // Air behind the start, material just in front of it.
                let back = if !inside(-0.5 * d) && inside(0.5 * d) { d } else { 0.0 };
                // Air just past the far end, material just before it.
                let front = if inside(length - 0.5 * d) && !inside(length + 0.5 * d) { d } else { 0.0 };
                (back, front)
            }
            FeatureOp::NewBody => (0.0, 0.0),
        };
        if back == 0.0 && front == 0.0 {
            return (start, length);
        }
        if travel > 0.0 {
            (start - back, length + back + front)
        } else {
            (start - front, length + back + front)
        }
    }

    /// Coarse meshes of the bodies a feature may act on, for probing where
    /// material is.
    fn target_meshes(&self, targets: &[String]) -> Vec<EvalMesh> {
        (0..self.bodies.len())
            .filter(|&i| self.targeted(i, targets))
            .map(|i| tessellate_solid(&self.bodies[i].solid, 0.01 * solid_bbox_diagonal(&self.bodies[i].solid)))
            .collect()
    }

    /// Span of an extrusion that stops at `target`: exact for a parallel
    /// plane, and trimmed by the plane otherwise.
    fn span_to_plane(
        &self,
        name: &str,
        regions: &[Region],
        frame: &Frame,
        target: &Frame,
    ) -> CadResult<(f64, f64, bool, Option<Frame>)> {
        let c = frame.z.dot(target.z);
        if c.abs() > 1.0 - 1.0e-9 {
            let dist = target.signed_distance(frame.origin) / -c;
            // `dist` is how far along +z the plane lies from the sketch.
            if dist.abs() < 1.0e-9 {
                return Err(err(name, "the target plane is the sketch plane itself"));
            }
            return Ok(if dist > 0.0 { (0.0, dist, false, None) } else { (dist, -dist, true, None) });
        }
        if c.abs() < 1.0e-9 {
            return Err(err(name, "the extrusion runs parallel to the target plane and never reaches it"));
        }
        let mut ts = Vec::new();
        for r in regions {
            for s in &r.outer.segs {
                let p = frame.to_world(s.geom.start());
                ts.push(-target.signed_distance(p) / c);
            }
        }
        let pos = ts.iter().all(|t| *t > 0.0);
        let neg = ts.iter().all(|t| *t < 0.0);
        if !pos && !neg {
            return Err(err(name, "the target plane crosses the profile, so there is no single side to extrude toward"));
        }
        let reach = ts.iter().fold(0.0_f64, |m, t| m.max(t.abs()));
        let over = (reach * 0.05).max(1.0e-4);
        Ok(if pos {
            (0.0, reach + over, false, Some(*target))
        } else {
            (-(reach + over), reach + over, true, Some(*target))
        })
    }

    /// The plane of the nearest planar face an extrusion from `regions`
    /// would run into, in direction `dir` along the normal.
    fn next_face_plane(&self, name: &str, regions: &[Region], frame: &Frame, dir: f64, targets: &[String]) -> CadResult<Frame> {
        let tol = DEFAULT_MESH_TOLERANCE;
        let ray = frame.z * dir;
        let mut best: Option<(f64, usize, usize)> = None;
        for bi in 0..self.bodies.len() {
            if !self.targeted(bi, targets) {
                continue;
            }
            let mesh = tessellate_solid(&self.bodies[bi].solid, tol);
            for r in regions {
                let n = r.outer.segs.len();
                for (k, s) in r.outer.segs.iter().enumerate() {
                    // A point just inside the region beside each side:
                    // the interior is to the left of a counter-clockwise loop.
                    let m = s.geom.midpoint();
                    let next = r.outer.segs[(k + 1) % n].geom.start();
                    let dx = next[0] - s.geom.start()[0];
                    let dy = next[1] - s.geom.start()[1];
                    let l = (dx * dx + dy * dy).sqrt().max(1.0e-12);
                    let inset = 1.0e-4 * (1.0 + r.outer.extent());
                    let p2 = [m[0] - dy / l * inset, m[1] + dx / l * inset];
                    let o = frame.to_world(p2) + ray * 1.0e-9;
                    if let Some((t, tri)) = crate::measure::raycast(&mesh, [o.x, o.y, o.z], [ray.x, ray.y, ray.z]) {
                        let face = mesh.face_ids.get(tri).copied().unwrap_or(0) as usize;
                        if best.map_or(true, |(bt, _, _)| t < bt) {
                            best = Some((t, bi, face));
                        }
                    }
                }
            }
        }
        let Some((_, bi, fi)) = best else {
            return Err(err(name, "up_to_next: the extrusion meets no face in that direction"));
        };
        let faces = faces_of(&self.bodies[bi]);
        let f = &faces[fi];
        f.plane.ok_or_else(|| {
            err(name, format!("up_to_next stops at '{}', which is not planar; only planar targets are supported", f.name))
        })
    }

    // ── Revolve ─────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn revolve(
        &mut self,
        name: &str,
        sketch: &str,
        axis: &str,
        angle: &str,
        combine: FeatureOp,
        both_sides: bool,
        bodies: &[String],
    ) -> CadResult<Outcome> {
        let (sk, frame) = self.sketch(sketch)?;
        let profile = profile_of(&sk)?;
        let a = self.angle(angle)?;
        let (origin, dir) = self.resolve_axis(axis, Some((&sk, &frame)))?;
        let mut gens = Vec::new();
        for (ri, region) in profile.regions.iter().enumerate() {
            let (solid, names) = build::revolve(region, &frame, origin, dir, a, both_sides, name, ri)?;
            gens.push(Gen { solid, names, op: combine, prism: None });
        }
        self.apply(name, gens, bodies)
    }

    // ── Hole ────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn hole(
        &mut self,
        name: &str,
        sketch_point: &str,
        diameter: &str,
        depth: &str,
        cb_d: Option<&str>,
        cb_depth: Option<&str>,
        csk_d: Option<&str>,
        csk_angle: Option<&str>,
        tap_class: Option<&str>,
    ) -> CadResult<Outcome> {
        match (cb_d.is_some(), cb_depth.is_some()) {
            (true, false) => {
                return Err(err(name, "counterbore_diameter was given without counterbore_depth; both are required"))
            }
            (false, true) => {
                return Err(err(name, "counterbore_depth was given without counterbore_diameter; both are required"))
            }
            _ => {}
        }
        if csk_angle.is_some() && csk_d.is_none() {
            return Err(err(name, "countersink_angle was given without countersink_diameter"));
        }
        let mut out = Outcome::default();
        let (sk_name, spec) = parse_sketch_ref(sketch_point)?;
        let (sk, frame) = self.sketch(&sk_name)?;
        let p2 = sketch_point_by_spec(&sk, &spec)?;
        let r = self.len(diameter)? * 0.5;
        let depth_m = self.len(depth)?;
        if !(r > 0.0) || !(depth_m > 0.0) {
            return Err(err(name, "hole diameter and depth must both be positive"));
        }

        if let Some(tc) = tap_class {
            let (major, pitch) = parse_metric_tap(tc).ok_or_else(|| {
                err(name, format!("tap_class '{tc}' is not a metric thread like \"M6\" or \"M6x0.75\""))
            })?;
            let drill = major - pitch;
            let d_mm = 2.0 * r * 1000.0;
            if (d_mm - drill).abs() > 0.05 * drill {
                out.note(format!(
                    "diameter {d_mm:.2} mm is not the {tc} tap drill ({drill:.2} mm); the bore is cut as given"
                ));
            }
            out.note(format!(
                "cosmetic thread {tc} (M{major}x{pitch}): recorded for drawings and export, not modeled as a helix"
            ));
        }

        // Drill INTO the material, whichever side of the sketch plane it is
        // on: a sketch on a top face drills down, a sketch on the plane
        // under a part drills up. When the plane runs through the part, drill
        // along the normal, which is what every existing tree meant.
        let base = frame.to_world(p2);
        let (lo, hi) = self.extent_along(base, frame.z, &[]).ok_or_else(|| err(name, "there is no body to drill"))?;
        let eps = 1.0e-9 * (1.0 + self.scale());
        let into = if hi <= eps && lo < -eps { -1.0 } else { 1.0 };
        let hf = if into > 0.0 { Frame { origin: base, ..frame } } else { Frame { origin: base, ..frame }.reversed() };
        let (lo2, hi2) = if into > 0.0 { (lo, hi) } else { (-hi, -lo) };

        let overcut = (depth_m * 0.05).max(1.0e-4);
        let through = depth_m >= hi2 - eps;
        let (start, len) = if through {
            let s = lo2.min(0.0) - overcut;
            (s, (hi2 - s) + overcut)
        } else {
            (-overcut, depth_m + overcut)
        };
        let far = if through { "exit" } else { "floor" };
        let (mut cutter, mut names) = build::cylinder(&hf, r, start, len, name, "entry", far)?;
        let tol = self.lineage_tol();
        let seq = self.seq;

        let union_into = |cutter: &mut Solid, names: &mut Vec<String>, extra: (Solid, Vec<String>), what: &str| -> CadResult<()> {
            let s1 = vec![seq; names.len()];
            let s2 = vec![seq; extra.1.len()];
            let u = boolean_or(cutter, &extra.0).ok_or_else(|| {
                err(name, format!("could not combine the {what} with the bore; the hole was not cut rather than cut without it"))
            })?;
            let (n, _) = inherit_names(
                &u,
                &[Named { solid: &*cutter, names: &names[..], seq: &s1 }, Named { solid: &extra.0, names: &extra.1, seq: &s2 }],
                tol,
                name,
            );
            *cutter = u;
            *names = n;
            Ok(())
        };

        if let (Some(d), Some(dp)) = (cb_d, cb_depth) {
            let cb_r = self.len(d)? * 0.5;
            let cb_depth_m = self.len(dp)?;
            if !(cb_r > r) {
                return Err(err(name, "counterbore_diameter must be larger than the hole diameter"));
            }
            // Staggered overcut so its top is not coplanar with the bore's.
            let cb = build::cylinder(&hf, cb_r, -2.0 * overcut, cb_depth_m + 2.0 * overcut, &format!("{name}.counterbore"), "entry", "floor")?;
            union_into(&mut cutter, &mut names, cb, "counterbore")?;
        }
        if let Some(d) = csk_d {
            let csk_r = self.len(d)? * 0.5;
            let angle = match csk_angle {
                Some(a) => self.angle(a)?,
                None => 90.0_f64.to_radians(),
            };
            if !(angle > 0.0 && angle < PI) {
                return Err(err(name, "countersink_angle must be between 0 and 180 degrees"));
            }
            if !(csk_r > r) {
                return Err(err(name, "countersink_diameter must be larger than the hole diameter"));
            }
            let tan = (angle * 0.5).tan();
            let lift = 3.0 * overcut;
            let r_top = csk_r + lift * tan;
            // Where the cone meets the bore, and where the bore ends.
            let meet = (csk_r - r) / tan;
            let floor = start + len;
            if !(floor > meet) {
                return Err(err(
                    name,
                    "the countersink reaches past the end of the hole; make it smaller or the hole deeper",
                ));
            }
            // End the cone inside the bore: past the circle where it meets
            // the bore, short of the axis and of the bore's floor.
            let bottom = meet + (0.5 * r / tan).min(0.5 * (floor - meet));
            let r_bottom = csk_r - bottom * tan;
            let cone = build::countersink_frustum(&hf, r_top, -lift, r_bottom, bottom, name)?;
            union_into(&mut cutter, &mut names, cone, "countersink")?;
            out.note(format!("countersink {:.1} deg to {:.2} mm", angle.to_degrees(), 2.0 * csk_r * 1000.0));
        }

        let o = self.apply(name, vec![Gen { solid: cutter, names, op: FeatureOp::Subtract, prism: None }], &[])?;
        out.absorb(o);
        Ok(out)
    }

    // ── Mirror / Pattern ────────────────────────────────────────────

    fn mirror(&mut self, name: &str, plane: &str, features: &[String], combine: FeatureOp) -> CadResult<Outcome> {
        let pf = self.resolve_frame(plane)?;
        let m = reflection_about(&pf);
        let sources = if features.is_empty() { self.bodies_as_gens() } else { self.gens_of(name, features)? };
        if sources.is_empty() {
            return Err(err(name, "there is nothing to mirror yet"));
        }
        let gens = sources
            .into_iter()
            .map(|g| {
                let mut s = builder::transformed(&g.solid, m);
                // A reflection has determinant -1; `transformed` keeps each
                // face's orientation flag, so the copy comes out inside-out
                // until its faces are inverted. Left as is, a mirrored body
                // has negative volume, and `boolean_not` (which inverts its
                // second operand) turns a mirrored cut into an intersection.
                s.not();
                Gen {
                    solid: s,
                    names: g.names.iter().map(|n| format!("{}@{name}", base_name(n))).collect(),
                    op: effective_op(g.op, combine),
                    prism: None,
                }
            })
            .collect();
        self.apply(name, gens, &[])
    }

    #[allow(clippy::too_many_arguments)]
    fn pattern(
        &mut self,
        name: &str,
        kind: crate::PatternKind,
        features: &[String],
        count: u32,
        spacing: Option<&str>,
        direction: Option<[f64; 3]>,
        axis: Option<&str>,
        angle: Option<&str>,
        direction_ref: Option<&str>,
        combine: FeatureOp,
    ) -> CadResult<Outcome> {
        if features.is_empty() && matches!(combine, FeatureOp::Subtract | FeatureOp::Intersect) {
            return Err(err(
                name,
                "a subtractive pattern must name the cutting feature in `features` (e.g. [\"Hole1\"]); \
                 with none given the whole part would be its own cutter",
            ));
        }
        let sources = if features.is_empty() { self.bodies_as_gens() } else { self.gens_of(name, features)? };
        if sources.is_empty() {
            return Err(err(name, "there is nothing to pattern yet"));
        }
        let transforms: Vec<Matrix4> = match kind {
            crate::PatternKind::Linear => {
                let dir = match direction_ref {
                    Some(e) => {
                        let (_, edge) = self.find_edge(e)?;
                        if !edge.straight {
                            return Err(err(name, format!("direction_ref '{e}' is not a straight edge")));
                        }
                        Vector3::new(edge.b[0] - edge.a[0], edge.b[1] - edge.a[1], edge.b[2] - edge.a[2])
                    }
                    None => {
                        let d = direction.unwrap_or([1.0, 0.0, 0.0]);
                        Vector3::new(d[0], d[1], d[2])
                    }
                };
                if !(dir.magnitude() > 1.0e-12) {
                    return Err(err(name, "pattern direction has zero length"));
                }
                let unit = dir.normalize();
                let step = match spacing {
                    Some(s) => self.len(s)?,
                    None => return Err(err(name, "a linear pattern needs `spacing`")),
                };
                (1..count).map(|i| Matrix4::from_translation(unit * (step * i as f64))).collect()
            }
            crate::PatternKind::Circular => {
                let (origin, dir) = self.resolve_axis(axis.unwrap_or("y"), None)?;
                let total = match angle {
                    Some(a) => self.angle(a)?,
                    None => TAU,
                };
                let step = if count < 2 {
                    0.0
                } else if (total.abs() - TAU).abs() < 1e-6 {
                    total / count as f64
                } else {
                    total / (count - 1) as f64
                };
                (1..count)
                    .map(|i| {
                        Matrix4::from_translation(origin.to_vec())
                            * Matrix4::from_axis_angle(dir, Rad(step * i as f64))
                            * Matrix4::from_translation(-origin.to_vec())
                    })
                    .collect()
            }
            crate::PatternKind::Path => {
                return Err(CadError::NotImplemented("Pattern path_kind = \"path\" is not implemented".into()))
            }
            crate::PatternKind::Sketch => {
                return Err(CadError::NotImplemented("Pattern path_kind = \"sketch\" is not implemented".into()))
            }
        };
        // `count` includes the seed, as in every mainstream CAD package; the
        // seed is already in the part, so the work is the copies 1..count.
        let mut gens = Vec::new();
        for (k, m) in transforms.iter().enumerate() {
            for g in &sources {
                gens.push(Gen {
                    solid: builder::transformed(&g.solid, *m),
                    names: g.names.iter().map(|n| format!("{}@{name}.{}", base_name(n), k + 2)).collect(),
                    op: effective_op(g.op, combine),
                    prism: None,
                });
            }
        }
        let total = gens.len();
        if total == 0 {
            let mut o = Outcome::default();
            o.note("count 1: the seed only, nothing to add");
            return Ok(o);
        }
        let mut out = self.apply(name, gens, &[])?;
        out.note(format!("{} total ({} copies + seed)", count, count.saturating_sub(1)));
        Ok(out)
    }

    // ── Boolean / Split ─────────────────────────────────────────────

    fn body_index(&self, feature: &str, name: &str) -> CadResult<usize> {
        self.bodies.iter().position(|b| b.name == name).ok_or_else(|| {
            let known: Vec<&str> = self.bodies.iter().map(|b| b.name.as_str()).collect();
            err(feature, format!("no body named '{name}'. Bodies: [{}]", known.join(", ")))
        })
    }

    fn boolean(&mut self, name: &str, target: &str, op: crate::BooleanOp, tools: &[String], keep_tools: bool) -> CadResult<Outcome> {
        let fop = match op {
            crate::BooleanOp::Union => FeatureOp::Add,
            crate::BooleanOp::Difference => FeatureOp::Subtract,
            crate::BooleanOp::Intersect => FeatureOp::Intersect,
        };
        if tools.is_empty() {
            // Legacy form: `target` is the TOOL, applied to every other body.
            let ti = self.body_index(name, target)?;
            let tool = self.bodies[ti].clone();
            if !keep_tools {
                self.bodies.remove(ti);
            }
            let others: Vec<String> = self.bodies.iter().filter(|b| b.name != tool.name).map(|b| b.name.clone()).collect();
            if others.is_empty() {
                return Err(err(name, format!("body '{target}' has nothing to combine with")));
            }
            let g = Gen { solid: tool.solid, names: tool.face_names, op: fop, prism: None };
            return self.apply(name, vec![g], &others);
        }
        let _ = self.body_index(name, target)?;
        let mut gens = Vec::new();
        for t in tools {
            let ti = self.body_index(name, t)?;
            let b = &self.bodies[ti];
            gens.push(Gen { solid: b.solid.clone(), names: b.face_names.clone(), op: fop, prism: None });
        }
        if !keep_tools {
            self.bodies.retain(|b| !tools.contains(&b.name));
        }
        self.apply(name, gens, &[target.to_string()])
    }

    fn split(&mut self, name: &str, plane: &str) -> CadResult<Outcome> {
        let pf = self.resolve_frame(plane)?;
        let tol = self.lineage_tol();
        let extent = 10.0 * self.scale();
        let (neg_slab, neg_names) = build::halfspace_slab(&pf, extent, name)?;
        let (pos_slab, pos_names) = build::halfspace_slab(&pf.reversed(), extent, name)?;
        let slab_seq_n = vec![self.seq; neg_names.len()];
        let slab_seq_p = vec![self.seq; pos_names.len()];
        let mut out = Outcome::default();
        let mut added: Vec<(usize, Body)> = Vec::new();
        let mut touched = 0;
        for i in 0..self.bodies.len() {
            let Some((lo, hi)) = topology::extent_along(&self.bodies[i].solid, pf.origin, pf.z) else { continue };
            let eps = 1.0e-9 * (1.0 + self.scale());
            if lo >= -eps || hi <= eps {
                continue;
            }
            let body = self.bodies[i].clone();
            let pos = boolean_and(&body.solid, &pos_slab)
                .ok_or_else(|| err(name, format!("could not split body '{}' (positive side)", body.name)))?;
            let neg = boolean_and(&body.solid, &neg_slab)
                .ok_or_else(|| err(name, format!("could not split body '{}' (negative side)", body.name)))?;
            let (pn, ps) = inherit_names(&pos, &[body.named(), Named { solid: &pos_slab, names: &pos_names, seq: &slab_seq_p }], tol, name);
            let (nn, ns) = inherit_names(&neg, &[body.named(), Named { solid: &neg_slab, names: &neg_names, seq: &slab_seq_n }], tol, name);
            self.bodies[i] = Body { name: body.name.clone(), solid: pos, face_names: pn, face_seq: ps, prism: None };
            added.push((i + 1, Body { name: format!("{}.{name}", body.name), solid: neg, face_names: nn, face_seq: ns, prism: None }));
            touched += 1;
        }
        if touched == 0 {
            return Err(err(name, "the split plane does not cross any body"));
        }
        for (at, b) in added.into_iter().rev() {
            out.note(format!("new body '{}'", b.name));
            self.bodies.insert(at, b);
        }
        Ok(out)
    }

    // ── Fillet / Chamfer ────────────────────────────────────────────

    fn blend(&mut self, name: &str, edges: &[String], kind: crate::blend::BlendKind) -> CadResult<Outcome> {
        if edges.is_empty() {
            return Err(err(name, "name at least one edge; list edge names with cad_list_topology"));
        }
        let legacy: Vec<&String> = edges.iter().filter(|e| is_legacy_edge_ref(e)).collect();
        if !legacy.is_empty() {
            if legacy.len() != edges.len() {
                return Err(err(name, "do not mix legacy positional references (\"Extrude1/edge-0\") with edge names"));
            }
            // The positional form never identified an edge. Keep the old
            // visual rounding so existing parts look as they did, and say
            // exactly what it is.
            let r = kind.size();
            self.legacy_round = self.legacy_round.max(r);
            let mut o = Outcome::default();
            o.note(format!(
                "mesh-edge {} r={r:.4}m on {} legacy edge reference(s): visual only, the solid is unchanged. \
                 Use edge names from cad_list_topology for a real {}.",
                kind.label(),
                edges.len(),
                kind.label()
            ));
            return Ok(o);
        }
        // Resolve every edge, then group by body.
        let mut by_body: Vec<(usize, Vec<(usize, EdgeInfo)>)> = Vec::new();
        for (k, e) in edges.iter().enumerate() {
            let (bi, info) = self.find_edge(e)?;
            match by_body.iter_mut().find(|(b, _)| *b == bi) {
                Some((_, v)) => v.push((k, info)),
                None => by_body.push((bi, vec![(k, info)])),
            }
        }
        let mut out = Outcome::default();
        let tol = self.lineage_tol();
        for (bi, list) in by_body {
            let (body, notes, degraded) = crate::blend::blend_body(&self.bodies[bi], &list, name, kind, self.seq, tol)?;
            self.bodies[bi] = body;
            for n in notes {
                out.note(n);
            }
            for d in degraded {
                out.degrade(d);
            }
        }
        Ok(out)
    }

    // ── Shell ───────────────────────────────────────────────────────

    fn shell(&mut self, name: &str, open_faces: &[String], t: f64) -> CadResult<Outcome> {
        if !(t > 0.0) {
            return Err(err(name, "wall_thickness must be positive"));
        }
        let mut open_in: Vec<(usize, String)> = Vec::new();
        for f in open_faces {
            let (bi, info) = self.find_face(f).ok_or_else(|| {
                err(name, format!("no face named '{f}'. Planar faces include: [{}]", self.face_names_hint()))
            })?;
            open_in.push((bi, info.name));
        }
        let bi = match (open_in.first(), self.bodies.len()) {
            (Some((b, _)), _) => {
                if open_in.iter().any(|(x, _)| x != b) {
                    return Err(err(name, "all open faces of one shell must belong to the same body"));
                }
                *b
            }
            (None, 1) => 0,
            (None, 0) => return Err(err(name, "no body to shell")),
            (None, _) => return Err(err(name, "the part has several bodies; name an open face to say which one to shell")),
        };
        let body = self.bodies[bi].clone();
        let mut out = Outcome::default();
        let tol = self.lineage_tol();
        let seq = self.seq;

        if let Some(prism) = body.prism.clone() {
            let cap = |which: &str| format!("{}.{which}", prism.feature);
            let is = |n: &str, which: &str| base_name(n) == cap(which) || base_name(n).starts_with(&format!("{}.r", cap(which)));
            let mut start_open = false;
            let mut end_open = false;
            for (_, n) in &open_in {
                if is(n, "cap_start") {
                    start_open = true;
                } else if is(n, "cap_end") {
                    end_open = true;
                } else {
                    return Err(CadError::NotImplemented(format!(
                        "opening '{n}' is not implemented: a shell can open the end caps of the extrusion that \
                         made the body ('{}' / '{}'), not its side walls",
                        cap("cap_start"),
                        cap("cap_end")
                    )));
                }
            }
            // Which cap sits at the lower end along the frame normal.
            let (lower_open, upper_open) = if prism.reversed { (end_open, start_open) } else { (start_open, end_open) };
            let over = (prism.length * 0.05).max(1.0e-4);
            let lo = if lower_open { prism.start - over } else { prism.start + t };
            let hi = if upper_open { prism.start + prism.length + over } else { prism.start + prism.length - t };
            if !(hi - lo > 1.0e-9) {
                return Err(err(name, format!("a {t:.6} m wall leaves no cavity in a {:.6} m tall body", prism.length)));
            }
            let inner_name = format!("{name}.inner");
            let mut inner_parts: Vec<(Solid, Vec<String>)> = Vec::new();
            for (ri, region) in prism.regions.iter().enumerate() {
                let inner = build::offset_region(region, -t).map_err(|e| err(name, format!("wall too thick for the profile: {e}")))?;
                inner_parts.push(build::prism(&inner, &prism.frame, lo, hi - lo, &inner_name, ri, false)?);
            }
            if !lower_open && !upper_open {
                // A closed cavity is a second, inward-facing boundary shell
                // of the same solid. No boolean can make one (the cavity
                // shares no surface with the outside), and none is needed.
                let mut shells: Vec<Shell> = body.solid.boundaries().clone();
                let mut names = body.face_names.clone();
                let mut seqs = body.face_seq.clone();
                for (mut s, n) in inner_parts {
                    s.not();
                    seqs.extend(std::iter::repeat(seq).take(n.len()));
                    names.extend(n);
                    shells.extend(s.into_boundaries());
                }
                let solid = Solid::try_new(shells).map_err(|e| err(name, format!("could not form the cavity: {e}")))?;
                self.bodies[bi] = Body { name: body.name, solid, face_names: names, face_seq: seqs, prism: None };
                out.note(format!("closed shell, wall {:.3} mm, fully enclosed cavity", t * 1000.0));
                return Ok(out);
            }
            let mut current = body.clone();
            for (inner, inner_names) in inner_parts {
                let r = boolean_not(&current.solid, &inner)
                    .ok_or_else(|| err(name, "could not cut the cavity (the kernel returned no result)"))?;
                let s2 = vec![seq; inner_names.len()];
                let (n, q) = inherit_names(&r, &[current.named(), Named { solid: &inner, names: &inner_names, seq: &s2 }], tol, name);
                if let Some(p) = &mut current.prism {
                    p.history.push(Replay { op: FeatureOp::Subtract, tool: inner.clone(), tool_names: inner_names.clone(), tool_seq: seq });
                }
                current.solid = r;
                current.face_names = n;
                current.face_seq = q;
            }
            self.bodies[bi] = current;
            out.note(format!("shell wall {:.3} mm", t * 1000.0));
            return Ok(out);
        }

        // Not a prism: the scaled-inner-body approximation, which is only
        // right for boxes. Say so.
        if !open_in.is_empty() {
            return Err(CadError::NotImplemented(
                "shelling a body that is not a single extruded profile, with named open faces, is not implemented"
                    .into(),
            ));
        }
        let (bmin, bmax) = topology::bounds(&body.solid).ok_or_else(|| err(name, "body has no vertices"))?;
        let (dx, dy, dz) = (bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]);
        if dx <= 2.0 * t || dy <= 2.0 * t || dz <= t {
            return Err(err(name, format!("wall {t:.4} m too thick for body {dx:.4}x{dy:.4}x{dz:.4} m")));
        }
        let over = (dz * 0.05).max(1.0e-4);
        let (sx, sy) = ((dx - 2.0 * t) / dx, (dy - 2.0 * t) / dy);
        let sz = ((bmax[2] + over) - (bmin[2] + t)) / dz;
        let anchor = Vector3::new((bmin[0] + bmax[0]) * 0.5, (bmin[1] + bmax[1]) * 0.5, bmin[2]);
        let m = Matrix4::from_translation(Vector3::new(0.0, 0.0, t))
            * Matrix4::from_translation(anchor)
            * Matrix4::from_nonuniform_scale(sx, sy, sz)
            * Matrix4::from_translation(-anchor);
        let inner = builder::transformed(&body.solid, m);
        let inner_names: Vec<String> = body.face_names.iter().map(|n| format!("{name}.inner.{}", base_name(n))).collect();
        let r = boolean_not(&body.solid, &inner).ok_or_else(|| err(name, "Shell: boolean difference with the inner body failed"))?;
        let s2 = vec![seq; inner_names.len()];
        let (n, q) = inherit_names(&r, &[body.named(), Named { solid: &inner, names: &inner_names, seq: &s2 }], tol, name);
        self.bodies[bi] = Body { name: body.name, solid: r, face_names: n, face_seq: q, prism: None };
        out.degrade(format!(
            "approximate open-top shell t={t:.4} m: the body is not a single extruded profile, so the cavity is a \
             scaled copy of the body, which is exact only for boxes"
        ));
        Ok(out)
    }

    // ── Sweep / Loft ────────────────────────────────────────────────

    fn sweep(&mut self, name: &str, profile: &str, path: &str, combine: FeatureOp) -> CadResult<Outcome> {
        let (psk, _pframe) = self.sketch(profile)?;
        let (qsk, qframe) = self.sketch(path)?;
        let prof = profile_of(&psk)?;
        let pts = path_points(&qsk, &qframe)?;
        let radius = prof.regions.iter().map(|r| r.outer.extent()).fold(0.0_f64, f64::max) * 0.5;
        // Rotation-minimizing frames: carry the profile's x axis from one
        // segment to the next, so consecutive segments line up instead of
        // twisting to whatever a world axis dictates.
        let mut x_prev: Option<Vector3> = None;
        let mut parts: Vec<(Solid, Vec<String>)> = Vec::new();
        let nseg = pts.len() - 1;
        for (k, w) in pts.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let dvec = b - a;
            let len = dvec.magnitude();
            if len < 1.0e-12 {
                continue;
            }
            let z = dvec / len;
            let mut f = Frame::from_origin_normal(a, z).ok_or_else(|| err(name, "degenerate path segment"))?;
            if let Some(xp) = x_prev {
                let x = xp - z * xp.dot(z);
                if x.magnitude() > 1.0e-9 {
                    let x = x.normalize();
                    f = Frame { origin: a, x, y: z.cross(x), z };
                }
            }
            x_prev = Some(f.x);
            // Overlap consecutive segments so each union has real
            // intersection curves; shapeops returns nothing for solids that
            // only touch.
            let e0 = if k == 0 { 0.0 } else { radius };
            let e1 = if k + 1 == nseg { 0.0 } else { radius };
            for (ri, region) in prof.regions.iter().enumerate() {
                let seg_name = format!("{name}.seg{}", k + 1);
                parts.push(build::prism(region, &f, -e0, len + e0 + e1, &seg_name, ri, false)?);
            }
        }
        let Some((mut acc, mut acc_names)) = parts.first().cloned() else {
            return Err(err(name, "every path segment has zero length"));
        };
        let tol = self.lineage_tol();
        for (s, n) in parts.iter().skip(1) {
            let s1 = vec![self.seq; acc_names.len()];
            let s2 = vec![self.seq; n.len()];
            let u = boolean_or(&acc, s).ok_or_else(|| {
                err(name, "could not join consecutive path segments; a path with very sharp corners can defeat the kernel")
            })?;
            let (nn, _) = inherit_names(&u, &[Named { solid: &acc, names: &acc_names, seq: &s1 }, Named { solid: s, names: n, seq: &s2 }], tol, name);
            acc = u;
            acc_names = nn;
        }
        let mut out = self.apply(name, vec![Gen { solid: acc, names: acc_names, op: combine, prism: None }], &[])?;
        if nseg > 1 {
            out.note("segmented sweep: path corners are joined by overlapping segments, not mitred");
        }
        Ok(out)
    }

    fn loft(&mut self, name: &str, profiles: &[String], combine: FeatureOp) -> CadResult<Outcome> {
        if profiles.len() < 2 {
            return Err(err(name, "a loft needs at least two profile sketches"));
        }
        let mut sections: Vec<(Region, Frame)> = Vec::new();
        for p in profiles {
            let (sk, frame) = self.sketch(p)?;
            let prof = profile_of(&sk)?;
            if prof.regions.len() != 1 || !prof.regions[0].holes.is_empty() {
                return Err(err(name, format!("loft section '{p}' must be a single closed outline without holes")));
            }
            sections.push((prof.regions[0].clone(), frame));
        }
        build::prepare_loft_sections(&mut sections).map_err(|e| err(name, e))?;
        let (solid, mut names) = build::loft(&sections, name)?;
        // Splitting edges to match section edge counts can give two side
        // faces one name; number them like any other duplicate.
        topology::disambiguate(&solid, &mut names);
        self.apply(name, vec![Gen { solid, names, op: combine, prism: None }], &[])
    }

    fn reference_plane(&mut self, name: &str, plane: &crate::ReferencePlane) -> CadResult<Outcome> {
        use crate::ReferencePlane::*;
        let frame = match plane {
            Offset { base, distance } => self.resolve_frame(base)?.offset(self.len(distance)?),
            ThreePoint { p1, p2, p3 } => Frame::from_three_points(
                Point3::new(p1[0], p1[1], p1[2]),
                Point3::new(p2[0], p2[1], p2[2]),
                Point3::new(p3[0], p3[1], p3[2]),
            )
            .ok_or_else(|| err(name, "the three points are collinear"))?,
            TangentFace { face } => {
                let (_, info) = self.find_face(face).ok_or_else(|| err(name, format!("no face named '{face}'")))?;
                info.plane.ok_or_else(|| err(name, format!("face '{face}' is not planar")))?
            }
            NormalToCurve { curve, t } => {
                let (_, e) = self.find_edge(curve)?;
                if !e.straight {
                    return Err(err(name, "normal_to_curve needs a straight edge"));
                }
                let a = Point3::new(e.a[0], e.a[1], e.a[2]);
                let b = Point3::new(e.b[0], e.b[1], e.b[2]);
                let p = a + (b - a) * t.clamp(0.0, 1.0);
                Frame::from_origin_normal(p, b - a).ok_or_else(|| err(name, "the edge has zero length"))?
            }
        };
        self.planes.insert(name.to_string(), frame);
        let mut o = Outcome::default();
        o.note(format!(
            "plane through ({:.4}, {:.4}, {:.4}) m, normal ({:.3}, {:.3}, {:.3})",
            frame.origin.x, frame.origin.y, frame.origin.z, frame.z.x, frame.z.y, frame.z.z
        ));
        Ok(o)
    }
}

/// A point strictly inside a region: the outline's centroid when that lies
/// in the material, otherwise a point just inside the first side.
fn interior_point(r: &Region) -> [f64; 2] {
    let poly = r.outer.polygon();
    let k = poly.len().max(1) as f64;
    let c = poly.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / k, a[1] + p[1] / k]);
    let in_material = |q: [f64; 2]| r.outer.contains(q) && !r.holes.iter().any(|h| h.contains(q));
    if in_material(c) {
        return c;
    }
    let s = &r.outer.segs[0].geom;
    let (a, b) = (s.start(), s.end());
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = (dx * dx + dy * dy).sqrt().max(1e-12);
    let m = s.midpoint();
    let inset = 1.0e-3 * r.outer.extent().max(1e-9);
    // Counter-clockwise outline: the material is to the left of travel.
    [m[0] - dy / l * inset, m[1] + dx / l * inset]
}

/// Would an extrusion of `regions` from the sketch plane, `reach` long in
/// direction `travel` (+1 along the normal, -1 against it), meet material?
/// Probed at an interior point of each region: inside a body just past the
/// plane, or a ray that reaches a body within `reach`.
fn meets_material(meshes: &[EvalMesh], regions: &[Region], frame: &Frame, travel: f64, reach: f64) -> bool {
    let along = frame.z * travel;
    let step = (1.0e-3 * reach).max(1.0e-7);
    regions.iter().any(|r| {
        let o = frame.to_world(interior_point(r)) + along * step;
        let (o, d) = ([o.x, o.y, o.z], [along.x, along.y, along.z]);
        meshes.iter().any(|m| {
            crate::measure::contains_point(m, o)
                || crate::measure::raycast(m, o, d).map_or(false, |(t, _)| t + step <= reach)
        })
    })
}

/// A pattern or mirror copy combines the way its source did (a hole's copy
/// cuts), unless the feature says otherwise.
fn effective_op(item: FeatureOp, combine: FeatureOp) -> FeatureOp {
    if combine == FeatureOp::Add {
        item
    } else {
        combine
    }
}

fn is_legacy_edge_ref(s: &str) -> bool {
    !s.contains('|')
        && s.rsplit_once("/edge-").map_or(false, |(_, n)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `"M6"` or `"M6x0.75"` -> (major mm, pitch mm). Coarse pitch from ISO 261
/// when none is given.
fn parse_metric_tap(s: &str) -> Option<(f64, f64)> {
    let t = s.trim().to_ascii_uppercase();
    let body = t.strip_prefix('M')?;
    let (major, pitch) = match body.split_once('X') {
        Some((a, b)) => (a.trim().parse::<f64>().ok()?, Some(b.trim().parse::<f64>().ok()?)),
        None => (body.trim().parse::<f64>().ok()?, None),
    };
    let coarse = [
        (1.6, 0.35), (2.0, 0.4), (2.5, 0.45), (3.0, 0.5), (4.0, 0.7), (5.0, 0.8), (6.0, 1.0), (8.0, 1.25),
        (10.0, 1.5), (12.0, 1.75), (14.0, 2.0), (16.0, 2.0), (20.0, 2.5), (24.0, 3.0), (30.0, 3.5),
    ];
    let pitch = match pitch {
        Some(p) => p,
        None => coarse.iter().find(|(d, _)| (d - major).abs() < 1e-9)?.1,
    };
    (major > pitch && pitch > 0.0).then_some((major, pitch))
}

/// World points of a path sketch's line chain.
fn path_points(path: &Sketch, frame: &Frame) -> CadResult<Vec<Point3>> {
    let mut pts: Vec<Point3> = Vec::new();
    for e in &path.entities {
        match e {
            SketchEntity::Line { p1, p2 } => {
                let a = frame.to_world(*p1);
                let b = frame.to_world(*p2);
                if pts.last().map_or(true, |p| (*p - a).magnitude() > 1e-9) {
                    pts.push(a);
                }
                pts.push(b);
            }
            SketchEntity::Point { p } => pts.push(frame.to_world(*p)),
            _ => {}
        }
    }
    if pts.len() < 2 {
        return Err(err("Sweep", "the path must be a chain of lines (or points) with at least two points"));
    }
    Ok(pts)
}

fn parse_sketch_ref(s: &str) -> CadResult<(String, String)> {
    let (sk, ent) = s.split_once('/').ok_or_else(|| err("sketch ref parse", format!("expected '<sketch>/<entity>', got '{s}'")))?;
    Ok((sk.to_string(), ent.to_string()))
}

/// Resolve `"point-2"` against a sketch. The index counts POINT entities,
/// so `point-2` is the third point regardless of lines drawn between them.
fn sketch_point_by_spec(sk: &Sketch, spec: &str) -> CadResult<[f64; 2]> {
    let points: Vec<[f64; 2]> = sk
        .entities
        .iter()
        .filter_map(|e| match e {
            SketchEntity::Point { p } => Some(*p),
            _ => None,
        })
        .collect();
    let idx = spec.rsplit_once('-').and_then(|(_, n)| n.parse::<usize>().ok());
    match idx {
        Some(i) => points.get(i).copied().ok_or_else(|| {
            err(
                "Hole",
                format!(
                    "'{spec}' refers to point {i}, but the sketch has {} point entit{} (valid indices 0..{}).",
                    points.len(),
                    if points.len() == 1 { "y" } else { "ies" },
                    points.len().saturating_sub(1)
                ),
            )
        }),
        None => Ok(points.first().copied().unwrap_or([0.0, 0.0])),
    }
}

pub(crate) fn resolve_length_meters(s: &str, vars: &HashMap<String, String>) -> CadResult<f64> {
    let q = crate::feature_tree::resolve_quantity_explained(s, vars)
        .map_err(|why| err("length lookup", format!("could not resolve '{s}': {why}")))?;
    match q.unit {
        crate::Unit::Length(_) => Ok(q.to_si()),
        // A bare number has no defensible reading as a length: metres and
        // millimetres are both plausible and 1000x apart, so refuse.
        crate::Unit::Scalar => Err(CadError::UnitMismatch {
            expected: format!(
                "a length with a unit, e.g. \"0.02 m\" or \"20 mm\" — '{s}' resolved to the unitless value {}",
                q.value
            ),
            got: "scalar (no unit)".into(),
        }),
        other => Err(CadError::UnitMismatch { expected: "length".into(), got: format!("{other:?}") }),
    }
}

pub(crate) fn resolve_angle_radians(s: &str, vars: &HashMap<String, String>) -> CadResult<f64> {
    let q = crate::feature_tree::resolve_quantity_explained(s, vars)
        .map_err(|why| err("angle lookup", format!("could not resolve '{s}': {why}")))?;
    match q.unit {
        crate::Unit::Angle(_) => Ok(q.to_si()),
        crate::Unit::Scalar => Ok(q.value * PI / 180.0), // bare numbers are degrees
        other => Err(CadError::UnitMismatch { expected: "angle".into(), got: format!("{other:?}") }),
    }
}

// ============================================================================
// Tessellation
// ============================================================================

/// Default deviation tolerance for tessellation, in metres.
pub const DEFAULT_MESH_TOLERANCE: f64 = 0.001;

/// Tessellate a truck `Solid` into flat triangle arrays, one face at a
/// time so every triangle knows its face (`face_ids` is the face's index
/// in `solid.face_iter()` order; `face_names` is left empty).
///
/// Surfaces are evaluated at unit scale: the same absolute scale floor
/// that breaks shapeops booleans on metre-native parts makes truck's
/// Newton projections diverge during triangulation of boolean results.
/// Positions are scaled back with plain f32 math. Everything runs under
/// `catch_unwind`: a kernel panic must never take down the editor.
pub fn tessellate_solid(solid: &Solid, tolerance: f64) -> EvalMesh {
    use truck_meshalgo::filters::{NormalFilters, OptimizingFilter};
    use truck_meshalgo::tessellation::{MeshableShape, RobustMeshableShape};

    let diagonal = solid_bbox_diagonal(solid);
    let scale = if diagonal > 1.0e-12 { 1.0 / diagonal } else { 1.0 };
    let solid = builder::transformed(solid, Matrix4::from_scale(scale));
    let tolerance = (tolerance * scale).max(1.0e-5);
    let inv_scale = (1.0 / scale) as f32;

    let Ok(meshed) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut meshed = solid.triangulation(tolerance);
        // The fast path needs boundary curves to ride exactly on their
        // surfaces; boolean output can violate that, and those faces come
        // back None. Retry with the projecting path before dropping them.
        if count_missing_faces(&meshed) > 0 {
            meshed = solid.robust_triangulation(tolerance);
        }
        meshed
    })) else {
        return EvalMesh::default();
    };

    let mut out = EvalMesh::default();
    let mut face_index = 0u32;
    for shell in meshed.boundaries() {
        for face in shell.face_iter() {
            if let Some(mut poly) = face.surface() {
                if !face.orientation() {
                    poly.invert();
                }
                poly.put_together_same_attrs(truck_base::tolerance::TOLERANCE);
                poly.remove_degenerate_faces().remove_unused_attrs();
                poly.add_naive_normals(false);
                let attrs = poly.attributes();
                let mut remap: HashMap<(usize, Option<usize>, Option<usize>), u32> = HashMap::new();
                for tri in poly.faces().triangle_iter() {
                    for v in tri {
                        let key = (v.pos, v.uv, v.nor);
                        let next = out.positions.len() as u32;
                        let idx = *remap.entry(key).or_insert_with(|| {
                            let p = attrs.positions[v.pos];
                            out.positions.push([p.x as f32 * inv_scale, p.y as f32 * inv_scale, p.z as f32 * inv_scale]);
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
                    out.face_ids.push(face_index);
                }
            }
            face_index += 1;
        }
    }
    out
}

/// Tessellate a body, with its face names.
pub fn tessellate_body(body: &Body, tolerance: f64) -> EvalMesh {
    let mut m = tessellate_solid(&body.solid, tolerance);
    m.face_names = body.face_names.clone();
    m.face_bodies = vec![body.name.clone(); body.face_names.len()];
    m
}

/// Concatenate meshes, offsetting indices and face ids.
pub fn merge_meshes(meshes: Vec<EvalMesh>) -> EvalMesh {
    let mut out = EvalMesh::default();
    for m in meshes {
        let base = out.positions.len() as u32;
        let face_base = out.face_names.len() as u32;
        out.positions.extend(m.positions);
        out.normals.extend(m.normals);
        out.uvs.extend(m.uvs);
        out.indices.extend(m.indices.iter().map(|i| i + base));
        out.face_ids.extend(m.face_ids.iter().map(|f| f + face_base));
        out.face_names.extend(m.face_names);
        out.face_bodies.extend(m.face_bodies);
    }
    out
}

/// Visual-only crease softening, kept solely for legacy positional
/// fillet/chamfer references (see `Model::blend`). It moves vertices and
/// blends normals; it does not change the solid.
fn soften_mesh_creases(mesh: &mut EvalMesh, radius: f32) {
    if mesh.positions.is_empty() || radius <= 0.0 {
        return;
    }
    let n = mesh.positions.len();
    let key_of = |p: [f32; 3]| -> (i32, i32, i32) {
        ((p[0] * 1.0e6).round() as i32, (p[1] * 1.0e6).round() as i32, (p[2] * 1.0e6).round() as i32)
    };
    let mut buckets: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    for i in 0..n {
        buckets.entry(key_of(mesh.positions[i])).or_default().push(i);
    }
    let mut min_p = [f32::INFINITY; 3];
    let mut max_p = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            min_p[a] = min_p[a].min(p[a]);
            max_p[a] = max_p[a].max(p[a]);
        }
    }
    let extent = ((max_p[0] - min_p[0]).powi(2) + (max_p[1] - min_p[1]).powi(2) + (max_p[2] - min_p[2]).powi(2))
        .sqrt()
        .max(1.0e-6);
    let max_off = radius.min(extent * 0.15).max(0.0);
    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    if new_normals.len() != n {
        new_normals = vec![[0.0, 1.0, 0.0]; n];
    }
    for indices in buckets.values() {
        if indices.len() < 2 {
            continue;
        }
        let mut avg = [0.0f32; 3];
        for &i in indices {
            for a in 0..3 {
                avg[a] += new_normals[i][a];
            }
        }
        let len = (avg[0] * avg[0] + avg[1] * avg[1] + avg[2] * avg[2]).sqrt();
        if len < 1.0e-8 {
            continue;
        }
        for a in avg.iter_mut() {
            *a /= len;
        }
        let mut min_dot = 1.0f32;
        for (a, &ia) in indices.iter().enumerate() {
            for &ib in indices.iter().skip(a + 1) {
                let (na, nb) = (new_normals[ia], new_normals[ib]);
                min_dot = min_dot.min(na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2]);
            }
        }
        if min_dot > 0.9 {
            continue;
        }
        let off = max_off * (1.0 - min_dot).clamp(0.0, 1.0) * 0.55;
        if off <= 1.0e-9 {
            continue;
        }
        for &i in indices {
            for a in 0..3 {
                new_positions[i][a] -= avg[a] * off;
            }
            let nn = new_normals[i];
            let mut b = [nn[0] * 0.45 + avg[0] * 0.55, nn[1] * 0.45 + avg[1] * 0.55, nn[2] * 0.45 + avg[2] * 0.55];
            let bl = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt().max(1.0e-8);
            for c in b.iter_mut() {
                *c /= bl;
            }
            new_normals[i] = b;
        }
    }
    mesh.positions = new_positions;
    mesh.normals = new_normals;
}

/// Count faces whose tessellation failed (`surface() == None`).
fn count_missing_faces<P, C, S: Clone>(meshed: &truck_topology::Solid<P, C, Option<S>>) -> usize {
    meshed.boundaries().iter().flat_map(|shell| shell.face_iter()).filter(|face| face.surface().is_none()).count()
}

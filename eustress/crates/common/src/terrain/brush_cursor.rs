//! Geometry of what the terrain tools draw at the cursor: the footprint ring
//! and strength disc on the ground, the brush volume's wireframe, the locked
//! plane, the local grid, height contours and the mirror planes (see
//! `docs/design/TERRAIN_TOOLS_UX.md`, section 5).
//!
//! Everything here is a pure function of the brush and a ground sampler
//! (`Fn(Vec2) -> Option<f32>`, `None` over a hole or off the terrain), so a
//! host builds meshes from the results and the tests check them without an
//! app. Segments are returned as point pairs, ready for a line-list mesh; a
//! segment with an end over no ground is left out rather than drawn to a
//! made-up height.
//!
//! Footprints are discs or squares. A square's "radius" is its half side, and
//! distance inside it is the Chebyshev distance (`max(|dx|, |dz|)`), so every
//! ring of the strength disc follows the square's outline and its falloff
//! fades evenly toward all four sides, the way the stroke applies it.

use bevy::math::{Quat, Vec2, Vec3};

use super::editor::falloff_weight;
use super::volume::CsgShape;

/// Samples around a disc footprint's ring (one every 4 degrees).
pub const RING_SEGMENTS: usize = 90;
/// Samples along each side of a square footprint's ring.
pub const SQUARE_SIDE_SEGMENTS: usize = 24;
/// How far above the ground the ring and disc float, in metres: enough to
/// clear the surface's own triangles, small enough to read as lying on it.
pub const GROUND_LIFT: f32 = 0.04;
/// Rings of the strength disc, centre excluded.
pub const DISC_RINGS: usize = 8;
/// Segments around the strength disc.
pub const DISC_SEGMENTS: usize = 48;
/// The strength disc's opacity at full strength and zero distance, so it
/// tints the ground without hiding it.
pub const DISC_MAX_ALPHA: f32 = 0.35;
/// Segments around each circle of a volume wireframe.
pub const WIRE_CIRCLE_SEGMENTS: usize = 64;
/// The most grid lines drawn along one axis; past it the grid steps up by a
/// factor of ten (see [`grid_step_for`]).
pub const MAX_GRID_LINES: usize = 80;
/// Grid samples along the side of the contour patch.
pub const CONTOUR_SAMPLES: usize = 48;
/// The most contour levels drawn through one patch; past it the interval
/// steps up (see [`contour_interval_for`]).
pub const MAX_CONTOUR_LEVELS: usize = 60;

/// A footprint's outline: a disc of `radius`, or a square of half side
/// `radius`, around `center`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub center: Vec2,
    pub radius: f32,
    pub square: bool,
}

impl Footprint {
    /// Distance from the centre in the footprint's own metric: Euclidean for
    /// a disc, Chebyshev for a square. The footprint is where this is at
    /// most `radius`.
    pub fn distance(&self, point: Vec2) -> f32 {
        let d = point - self.center;
        if self.square {
            d.x.abs().max(d.y.abs())
        } else {
            d.length()
        }
    }

    /// Unit-radius offset in direction `angle` that lies on the outline:
    /// the unit circle, or for a square the point where the ray at `angle`
    /// meets the square of half side 1.
    fn outline_direction(&self, angle: f32) -> Vec2 {
        let (sin, cos) = angle.sin_cos();
        let dir = Vec2::new(cos, sin);
        if self.square {
            dir / cos.abs().max(sin.abs()).max(1e-6)
        } else {
            dir
        }
    }

    /// The outline as a closed loop (the first point is not repeated), at
    /// `radius_scale` times the footprint's radius.
    pub fn outline(&self, radius_scale: f32) -> Vec<Vec2> {
        let r = self.radius * radius_scale;
        if self.square {
            let corners = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
            let mut points = Vec::with_capacity(4 * SQUARE_SIDE_SEGMENTS);
            for side in 0..4 {
                let (a, b) = (corners[side], corners[(side + 1) % 4]);
                for i in 0..SQUARE_SIDE_SEGMENTS {
                    let t = i as f32 / SQUARE_SIDE_SEGMENTS as f32;
                    points.push(self.center + a.lerp(b, t) * r);
                }
            }
            points
        } else {
            (0..RING_SEGMENTS)
                .map(|i| {
                    let angle = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                    self.center + self.outline_direction(angle) * r
                })
                .collect()
        }
    }
}

/// A closed loop of ground points lifted by [`GROUND_LIFT`], as segments:
/// each pair of neighbours with ground under both ends.
pub fn drape_loop(points: &[Vec2], ground: impl Fn(Vec2) -> Option<f32>) -> Vec<[Vec3; 2]> {
    let lifted: Vec<Option<Vec3>> = points
        .iter()
        .map(|p| ground(*p).map(|y| Vec3::new(p.x, y + GROUND_LIFT, p.y)))
        .collect();
    let n = lifted.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n).filter_map(|i| Some([lifted[i]?, lifted[(i + 1) % n]?])).collect()
}

/// A loop of points at height `y` (a locked plane), as segments.
pub fn flat_loop(points: &[Vec2], y: f32) -> Vec<[Vec3; 2]> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            [Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y)]
        })
        .collect()
}

/// The radius, as a fraction of the brush radius, at which the falloff
/// gives half the push: the brush's half-push ring. Solves
/// `falloff_weight(d, 1, falloff) = 0.5` by bisection, so it stays right for
/// any falloff curve `falloff_weight` implements. A hard brush (falloff 0)
/// pushes fully to its rim and returns 1.
pub fn half_push_fraction(falloff: f32) -> f32 {
    if !(falloff > 0.0) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if falloff_weight(mid, 1.0, falloff) > 0.5 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// A triangle mesh with a per-vertex opacity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlphaMesh {
    pub positions: Vec<Vec3>,
    pub alphas: Vec<f32>,
    pub indices: Vec<u32>,
}

/// The strength disc: a polar mesh over the footprint whose opacity at each
/// vertex is `strength` times the falloff there, times [`DISC_MAX_ALPHA`].
/// Vertices over no ground are dropped along with the triangles that use
/// them. With `plane` set the disc lies on that height instead of the
/// ground.
pub fn strength_disc(
    footprint: Footprint,
    strength: f32,
    falloff: f32,
    plane: Option<f32>,
    ground: impl Fn(Vec2) -> Option<f32>,
) -> AlphaMesh {
    let mut mesh = AlphaMesh::default();
    if !(footprint.radius > 0.0) {
        return mesh;
    }
    let strength = strength.clamp(0.0, 1.0);
    let height = |p: Vec2| match plane {
        Some(y) => Some(y + GROUND_LIFT),
        None => ground(p).map(|y| y + GROUND_LIFT),
    };
    let alpha_at = |fraction: f32| strength * falloff_weight(fraction, 1.0, falloff) * DISC_MAX_ALPHA;

    // Vertex slots: the centre, then ring by ring. `None` marks a vertex over
    // no ground, which no triangle may use.
    let mut slots: Vec<Option<u32>> = Vec::with_capacity(1 + DISC_RINGS * DISC_SEGMENTS);
    let push = |mesh: &mut AlphaMesh, p: Vec2, alpha: f32| -> Option<u32> {
        let y = height(p)?;
        mesh.positions.push(Vec3::new(p.x, y, p.y));
        mesh.alphas.push(alpha);
        Some((mesh.positions.len() - 1) as u32)
    };
    slots.push(push(&mut mesh, footprint.center, alpha_at(0.0)));
    for ring in 1..=DISC_RINGS {
        let fraction = ring as f32 / DISC_RINGS as f32;
        for segment in 0..DISC_SEGMENTS {
            let angle = segment as f32 / DISC_SEGMENTS as f32 * std::f32::consts::TAU;
            let p = footprint.center + footprint.outline_direction(angle) * footprint.radius * fraction;
            slots.push(push(&mut mesh, p, alpha_at(fraction)));
        }
    }
    let ring_slot = |ring: usize, segment: usize| 1 + (ring - 1) * DISC_SEGMENTS + segment % DISC_SEGMENTS;
    let mut tri = |a: Option<u32>, b: Option<u32>, c: Option<u32>| {
        if let (Some(a), Some(b), Some(c)) = (a, b, c) {
            mesh.indices.extend_from_slice(&[a, b, c]);
        }
    };
    for segment in 0..DISC_SEGMENTS {
        tri(slots[0], slots[ring_slot(1, segment + 1)], slots[ring_slot(1, segment)]);
    }
    for ring in 1..DISC_RINGS {
        for segment in 0..DISC_SEGMENTS {
            let (a, b) = (slots[ring_slot(ring, segment)], slots[ring_slot(ring, segment + 1)]);
            let (c, d) = (slots[ring_slot(ring + 1, segment)], slots[ring_slot(ring + 1, segment + 1)]);
            tri(a, b, d);
            tri(a, d, c);
        }
    }
    mesh
}

/// A circle of `radius` around `center` in the plane spanned by `u` and `v`,
/// as segments.
fn circle_segments(center: Vec3, radius: f32, u: Vec3, v: Vec3, out: &mut Vec<[Vec3; 2]>) {
    let point = |i: usize| {
        let angle = i as f32 / WIRE_CIRCLE_SEGMENTS as f32 * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        center + (u * cos + v * sin) * radius
    };
    for i in 0..WIRE_CIRCLE_SEGMENTS {
        out.push([point(i), point(i + 1)]);
    }
}

/// The twelve edges of a box of `half_extents` turned by `rotation` about
/// `center`.
fn box_edges(center: Vec3, half_extents: Vec3, rotation: Quat, out: &mut Vec<[Vec3; 2]>) {
    let corner = |sx: f32, sy: f32, sz: f32| center + rotation * (half_extents * Vec3::new(sx, sy, sz));
    for (sy, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        out.push([corner(-1.0, sy, sz), corner(1.0, sy, sz)]);
    }
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        out.push([corner(sx, -1.0, sz), corner(sx, 1.0, sz)]);
    }
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        out.push([corner(sx, sy, -1.0), corner(sx, sy, 1.0)]);
    }
}

/// A cylinder around its own Y axis turned by `rotation`: both end rings and
/// four rails between them.
fn cylinder_edges(center: Vec3, radius: f32, half_height: f32, rotation: Quat, out: &mut Vec<[Vec3; 2]>) {
    let (u, up, v) = (rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z);
    circle_segments(center - up * half_height, radius, u, v, out);
    circle_segments(center + up * half_height, radius, u, v, out);
    for dir in [u, v, -u, -v] {
        out.push([center - up * half_height + dir * radius, center + up * half_height + dir * radius]);
    }
}

/// The wireframe of the exact shape a Draw dab adds or carves: a sphere as
/// its three great circles, a box as its twelve edges, a cylinder as its two
/// end rings and four rails. With `clip` set, segments are cut to the side of
/// that height the clipped dab keeps (`keep_below` true keeps `y <= clip`),
/// and the clip's cross-section is not drawn: the locked plane shows it.
pub fn volume_wireframe(shape: &CsgShape, clip: Option<(f32, bool)>) -> Vec<[Vec3; 2]> {
    let mut segments = Vec::new();
    match *shape {
        CsgShape::Sphere { center, radius } => {
            circle_segments(center, radius, Vec3::X, Vec3::Z, &mut segments);
            circle_segments(center, radius, Vec3::X, Vec3::Y, &mut segments);
            circle_segments(center, radius, Vec3::Z, Vec3::Y, &mut segments);
        }
        CsgShape::AxisBox { center, half_extents } => box_edges(center, half_extents, Quat::IDENTITY, &mut segments),
        CsgShape::OrientedBox { center, half_extents, rotation } => box_edges(center, half_extents, rotation, &mut segments),
        CsgShape::Cylinder { center, radius, half_height } => {
            cylinder_edges(center, radius, half_height, Quat::IDENTITY, &mut segments)
        }
        CsgShape::OrientedCylinder { center, radius, half_height, rotation } => {
            cylinder_edges(center, radius, half_height, rotation, &mut segments)
        }
    }
    match clip {
        Some((y, keep_below)) => segments.into_iter().filter_map(|s| clip_segment(s, y, keep_below)).collect(),
        None => segments,
    }
}

/// The part of `segment` on the kept side of height `y`, or `None` when none
/// of it is.
fn clip_segment([a, b]: [Vec3; 2], y: f32, keep_below: bool) -> Option<[Vec3; 2]> {
    let kept = |p: Vec3| if keep_below { p.y <= y } else { p.y >= y };
    match (kept(a), kept(b)) {
        (true, true) => Some([a, b]),
        (false, false) => None,
        (ka, _) => {
            let t = (y - a.y) / (b.y - a.y);
            let cut = a.lerp(b, t.clamp(0.0, 1.0));
            Some(if ka { [a, cut] } else { [cut, b] })
        }
    }
}

/// The grid step drawn for a patch of `radius` at snap step `step`: `step`,
/// or ten times it (then a hundred, and so on) until at most
/// [`MAX_GRID_LINES`] lines cross the patch, so a wide brush over a fine
/// step never floods the view with lines.
pub fn grid_step_for(radius: f32, step: f32) -> f32 {
    let mut drawn = step.max(1e-3);
    while radius * 2.0 / drawn > MAX_GRID_LINES as f32 {
        drawn *= 10.0;
    }
    drawn
}

/// One segment of the local grid or of a contour, with an opacity at each
/// end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeSegment {
    pub a: Vec3,
    pub b: Vec3,
    pub alpha_a: f32,
    pub alpha_b: f32,
}

/// Opacity of a grid or contour point `distance` from the cursor on a patch
/// of `radius`: `peak` at the cursor, falling linearly to 0 at the edge.
fn fade(distance: f32, radius: f32, peak: f32) -> f32 {
    (peak * (1.0 - distance / radius.max(1e-6))).max(0.0)
}

/// The local grid (design section 4.7): lines at multiples of `step` (world
/// aligned, so they sit on the snap lattice) within `radius` of `center`,
/// draped on the ground or lying on `plane`, each end's opacity fading from
/// `minor_alpha` (`major_alpha` on every tenth line) at the cursor to 0 at
/// the edge. Each line is cut into pieces a quarter step long so it follows
/// the ground.
pub fn local_grid(
    center: Vec2,
    radius: f32,
    step: f32,
    plane: Option<f32>,
    minor_alpha: f32,
    major_alpha: f32,
    ground: impl Fn(Vec2) -> Option<f32>,
) -> Vec<FadeSegment> {
    let mut out = Vec::new();
    if !(radius > 0.0 && step > 0.0) {
        return out;
    }
    let step = grid_step_for(radius, step);
    let piece = (step * 0.25).max(radius / 64.0);
    let height = |p: Vec2| match plane {
        Some(y) => Some(y + GROUND_LIFT),
        None => ground(p).map(|y| y + GROUND_LIFT),
    };
    for axis in 0..2 {
        // Axis 0 draws lines of constant x, axis 1 lines of constant z.
        let (c_along, c_across) = if axis == 0 { (center.y, center.x) } else { (center.x, center.y) };
        let first = ((c_across - radius) / step).ceil() as i64;
        let last = ((c_across + radius) / step).floor() as i64;
        for k in first..=last {
            let across = k as f32 * step;
            let half = (radius * radius - (across - c_across).powi(2)).max(0.0).sqrt();
            if half <= 0.0 {
                continue;
            }
            let peak = if k.rem_euclid(10) == 0 { major_alpha } else { minor_alpha };
            let pieces = ((2.0 * half / piece).ceil() as usize).max(1);
            let at = |t: f32| {
                let along = c_along - half + 2.0 * half * t;
                if axis == 0 {
                    Vec2::new(across, along)
                } else {
                    Vec2::new(along, across)
                }
            };
            for i in 0..pieces {
                let (pa, pb) = (at(i as f32 / pieces as f32), at((i + 1) as f32 / pieces as f32));
                let (Some(ya), Some(yb)) = (height(pa), height(pb)) else { continue };
                out.push(FadeSegment {
                    a: Vec3::new(pa.x, ya, pa.y),
                    b: Vec3::new(pb.x, yb, pb.y),
                    alpha_a: fade(pa.distance(center), radius, peak),
                    alpha_b: fade(pb.distance(center), radius, peak),
                });
            }
        }
    }
    out
}

/// The contour interval drawn through a patch whose heights span `range`
/// metres at interval `step`: `step`, stepped up by fives and twos (5, 10,
/// 50, 100, ...) until at most [`MAX_CONTOUR_LEVELS`] levels cross it.
pub fn contour_interval_for(range: f32, step: f32) -> f32 {
    let mut interval = step.max(1e-3);
    let mut by_five = true;
    while range / interval > MAX_CONTOUR_LEVELS as f32 {
        interval *= if by_five { 5.0 } else { 2.0 };
        by_five = !by_five;
    }
    interval
}

/// Height contours (design section 4.7) through the disc of `radius` around
/// `center`: iso-lines at every multiple of the contour interval (see
/// [`contour_interval_for`]), traced by marching squares over a
/// [`CONTOUR_SAMPLES`] grid of ground heights, lifted by [`GROUND_LIFT`], with
/// opacity fading from `minor_alpha` (`major_alpha` on every fifth level) at
/// the cursor to 0 at the edge. Cells with a corner over no ground are
/// skipped.
pub fn contours(
    center: Vec2,
    radius: f32,
    step: f32,
    minor_alpha: f32,
    major_alpha: f32,
    ground: impl Fn(Vec2) -> Option<f32>,
) -> Vec<FadeSegment> {
    let mut out = Vec::new();
    if !(radius > 0.0 && step > 0.0) {
        return out;
    }
    let n = CONTOUR_SAMPLES;
    let cell = 2.0 * radius / n as f32;
    let origin = center - Vec2::splat(radius);
    let point = |i: usize, j: usize| origin + Vec2::new(i as f32 * cell, j as f32 * cell);
    let heights: Vec<Option<f32>> = (0..=n).flat_map(|j| (0..=n).map(move |i| (i, j))).map(|(i, j)| ground(point(i, j))).collect();
    let h = |i: usize, j: usize| heights[j * (n + 1) + i];
    let (lo, hi) = heights
        .iter()
        .flatten()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
    if !(hi > lo) {
        return out;
    }
    let interval = contour_interval_for(hi - lo, step);
    let first = (lo / interval).ceil() as i64;
    let last = (hi / interval).floor() as i64;
    for level_index in first..=last {
        let level = level_index as f32 * interval;
        let peak = if level_index.rem_euclid(5) == 0 { major_alpha } else { minor_alpha };
        for j in 0..n {
            for i in 0..n {
                let (Some(h00), Some(h10), Some(h01), Some(h11)) = (h(i, j), h(i + 1, j), h(i, j + 1), h(i + 1, j + 1))
                else {
                    continue;
                };
                // Corner order around the cell: (i,j), (i+1,j), (i+1,j+1), (i,j+1).
                let corners = [(point(i, j), h00), (point(i + 1, j), h10), (point(i + 1, j + 1), h11), (point(i, j + 1), h01)];
                let mut crossings: Vec<Vec2> = Vec::with_capacity(4);
                for e in 0..4 {
                    let (pa, ha) = corners[e];
                    let (pb, hb) = corners[(e + 1) % 4];
                    // Half-open test, so a level exactly on a corner height
                    // crosses each edge at most once.
                    if (ha < level) != (hb < level) {
                        let t = (level - ha) / (hb - ha);
                        crossings.push(pa.lerp(pb, t));
                    }
                }
                // Two crossings join; four (a saddle) join in edge order.
                for pair in crossings.chunks_exact(2) {
                    let (pa, pb) = (pair[0], pair[1]);
                    if pa.distance(center) > radius && pb.distance(center) > radius {
                        continue;
                    }
                    out.push(FadeSegment {
                        a: Vec3::new(pa.x, level + GROUND_LIFT, pa.y),
                        b: Vec3::new(pb.x, level + GROUND_LIFT, pb.y),
                        alpha_a: fade(pa.distance(center), radius, peak),
                        alpha_b: fade(pb.distance(center), radius, peak),
                    });
                }
            }
        }
    }
    out
}

/// The locked plane (design section 4.6): a flat disc of `radius` around
/// `center` at height `y`, as a triangle fan with opacity `fill_alpha`, its
/// rim as segments, and its grid lines at multiples of `step` clipped to
/// the disc.
pub fn plane_disc(center: Vec2, y: f32, radius: f32, step: f32, fill_alpha: f32) -> (AlphaMesh, Vec<[Vec3; 2]>, Vec<[Vec3; 2]>) {
    let footprint = Footprint { center, radius, square: false };
    let rim_points = footprint.outline(1.0);
    let mut fill = AlphaMesh::default();
    fill.positions.push(Vec3::new(center.x, y, center.y));
    fill.alphas.push(fill_alpha);
    for p in &rim_points {
        fill.positions.push(Vec3::new(p.x, y, p.y));
        fill.alphas.push(fill_alpha);
    }
    let n = rim_points.len() as u32;
    for i in 0..n {
        fill.indices.extend_from_slice(&[0, 1 + (i + 1) % n, 1 + i]);
    }
    let rim = flat_loop(&rim_points, y);
    let grid = local_grid(center, radius, step, Some(y - GROUND_LIFT), 1.0, 1.0, |_| None)
        .into_iter()
        .map(|s| [s.a, s.b])
        .collect();
    (fill, rim, grid)
}

/// Vertical mirror-plane markers near the cursor: for each mirror axis, a
/// line on the ground across the mirror plane through `origin`, reaching
/// `reach` either side of the cursor, draped on the ground in pieces.
/// `mirror_x` mirrors across the plane `x = origin.x` (a line of constant x),
/// `mirror_z` across `z = origin.y`.
pub fn mirror_lines(
    cursor: Vec2,
    origin: Vec2,
    reach: f32,
    mirror_x: bool,
    mirror_z: bool,
    ground: impl Fn(Vec2) -> Option<f32>,
) -> Vec<[Vec3; 2]> {
    let mut out = Vec::new();
    if !(reach > 0.0) {
        return out;
    }
    let pieces = 48usize;
    let mut line = |start: Vec2, end: Vec2| {
        let points: Vec<Vec2> = (0..=pieces).map(|i| start.lerp(end, i as f32 / pieces as f32)).collect();
        for pair in points.windows(2) {
            if let (Some(ya), Some(yb)) = (ground(pair[0]), ground(pair[1])) {
                out.push([
                    Vec3::new(pair[0].x, ya + GROUND_LIFT, pair[0].y),
                    Vec3::new(pair[1].x, yb + GROUND_LIFT, pair[1].y),
                ]);
            }
        }
    };
    if mirror_x {
        line(Vec2::new(origin.x, cursor.y - reach), Vec2::new(origin.x, cursor.y + reach));
    }
    if mirror_z {
        line(Vec2::new(cursor.x - reach, origin.y), Vec2::new(cursor.x + reach, origin.y));
    }
    out
}

/// Samples along each side of the Sea Level rectangle's outline.
pub const SEA_LEVEL_SIDE_SEGMENTS: usize = 32;
/// Grid samples along each side of the Sea Level rectangle for its shoreline.
pub const SHORELINE_SAMPLES: usize = 48;

/// What the Sea Level tool draws for its rectangle (design section 5).
#[derive(Clone, Debug, Default)]
pub struct SeaLevelBox {
    /// The rectangle draped on the ground.
    pub outline: Vec<[Vec3; 2]>,
    /// The water plane at the level.
    pub fill: AlphaMesh,
    /// The plane's rim.
    pub rim: Vec<[Vec3; 2]>,
    /// A post at each corner over ground, from the ground to the level.
    pub posts: Vec<[Vec3; 2]>,
    /// Where the level meets the ground inside the rectangle: the shore a
    /// fill would make, traced by marching squares over a
    /// [`SHORELINE_SAMPLES`] grid, lifted by [`GROUND_LIFT`].
    pub shoreline: Vec<[Vec3; 2]>,
}

/// The Sea Level tool's rectangle `a..b` (world XZ corners, either order),
/// and, with a `level`, its water plane (opacity `fill_alpha`), rim, corner
/// posts and shoreline. Nothing for a rectangle with a non-finite corner.
pub fn sea_level_box(
    a: Vec2,
    b: Vec2,
    level: Option<f32>,
    fill_alpha: f32,
    ground: impl Fn(Vec2) -> Option<f32>,
) -> SeaLevelBox {
    let mut out = SeaLevelBox::default();
    if !(a.is_finite() && b.is_finite()) {
        return out;
    }
    let (lo, hi) = (a.min(b), a.max(b));
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    let mut points = Vec::with_capacity(4 * SEA_LEVEL_SIDE_SEGMENTS);
    for i in 0..4 {
        let (from, to) = (corners[i], corners[(i + 1) % 4]);
        points.extend((0..SEA_LEVEL_SIDE_SEGMENTS).map(|s| from.lerp(to, s as f32 / SEA_LEVEL_SIDE_SEGMENTS as f32)));
    }
    out.outline = drape_loop(&points, &ground);
    let Some(level) = level.filter(|level| level.is_finite()) else {
        return out;
    };
    out.fill = AlphaMesh {
        positions: corners.iter().map(|c| Vec3::new(c.x, level, c.y)).collect(),
        alphas: vec![fill_alpha; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
    };
    out.rim = flat_loop(&corners, level);
    for c in corners {
        if let Some(y) = ground(c) {
            out.posts.push([Vec3::new(c.x, y + GROUND_LIFT, c.y), Vec3::new(c.x, level, c.y)]);
        }
    }

    let n = SHORELINE_SAMPLES;
    let span = hi - lo;
    if !(span.x > 0.0 && span.y > 0.0) {
        return out;
    }
    let point = |i: usize, j: usize| lo + span * Vec2::new(i as f32, j as f32) / n as f32;
    let heights: Vec<Option<f32>> = (0..=n).flat_map(|j| (0..=n).map(move |i| (i, j))).map(|(i, j)| ground(point(i, j))).collect();
    let h = |i: usize, j: usize| heights[j * (n + 1) + i];
    for j in 0..n {
        for i in 0..n {
            let (Some(h00), Some(h10), Some(h01), Some(h11)) = (h(i, j), h(i + 1, j), h(i, j + 1), h(i + 1, j + 1)) else {
                continue;
            };
            let cell = [(point(i, j), h00), (point(i + 1, j), h10), (point(i + 1, j + 1), h11), (point(i, j + 1), h01)];
            let mut crossings: Vec<Vec2> = Vec::with_capacity(4);
            for e in 0..4 {
                let (pa, ha) = cell[e];
                let (pb, hb) = cell[(e + 1) % 4];
                if (ha < level) != (hb < level) {
                    crossings.push(pa.lerp(pb, (level - ha) / (hb - ha)));
                }
            }
            for pair in crossings.chunks_exact(2) {
                out.shoreline.push([
                    Vec3::new(pair[0].x, level + GROUND_LIFT, pair[0].y),
                    Vec3::new(pair[1].x, level + GROUND_LIFT, pair[1].y),
                ]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sea_level_box_drapes_its_rectangle_and_draws_the_shore() {
        // Ground rising 1 m per metre east of x = 0: a 2 m level meets it at x = 2.
        let slope = |p: Vec2| Some(p.x.max(0.0));
        let empty = sea_level_box(Vec2::new(4.0, 3.0), Vec2::new(-4.0, -3.0), None, 0.2, slope);
        assert_eq!(empty.outline.len(), 4 * SEA_LEVEL_SIDE_SEGMENTS, "corners in either order");
        assert!(empty.fill.positions.is_empty() && empty.rim.is_empty() && empty.posts.is_empty());
        for [a, b] in &empty.outline {
            for p in [a, b] {
                assert!(p.x.abs() <= 4.0 + 1e-4 && p.z.abs() <= 3.0 + 1e-4, "{p} is off the rectangle");
                assert!((p.y - (p.x.max(0.0) + GROUND_LIFT)).abs() < 1e-4, "{p} is not on the ground");
            }
        }

        let with_level = sea_level_box(Vec2::new(-4.0, -3.0), Vec2::new(4.0, 3.0), Some(2.0), 0.2, slope);
        assert_eq!(with_level.fill.positions.len(), 4);
        assert!(with_level.fill.positions.iter().all(|p| p.y == 2.0));
        assert_eq!(with_level.fill.indices.len(), 6);
        assert_eq!(with_level.rim.len(), 4);
        assert_eq!(with_level.posts.len(), 4, "every corner stands on ground");
        assert!(!with_level.shoreline.is_empty());
        for [a, b] in &with_level.shoreline {
            for p in [a, b] {
                assert!((p.x - 2.0).abs() < 1e-3, "the shore at {p} is off x = 2");
                assert!((p.y - (2.0 + GROUND_LIFT)).abs() < 1e-5);
            }
        }

        // A level above all the ground has no shore; no ground, no outline.
        assert!(sea_level_box(Vec2::ZERO, Vec2::splat(1.0), Some(50.0), 0.2, slope).shoreline.is_empty());
        let air = sea_level_box(Vec2::ZERO, Vec2::splat(1.0), Some(1.0), 0.2, |_| None);
        assert!(air.outline.is_empty() && air.posts.is_empty() && air.shoreline.is_empty() && air.rim.len() == 4);
        assert!(sea_level_box(Vec2::new(f32::NAN, 0.0), Vec2::ONE, Some(1.0), 0.2, slope).rim.is_empty());
    }

    fn flat(y: f32) -> impl Fn(Vec2) -> Option<f32> {
        move |_| Some(y)
    }

    #[test]
    fn disc_outline_lies_on_the_circle_and_square_outline_on_the_square() {
        let disc = Footprint { center: Vec2::new(3.0, -2.0), radius: 5.0, square: false };
        let points = disc.outline(1.0);
        assert_eq!(points.len(), RING_SEGMENTS);
        assert!(points.iter().all(|p| (p.distance(disc.center) - 5.0).abs() < 1e-4));

        let square = Footprint { square: true, ..disc };
        let points = square.outline(1.0);
        assert_eq!(points.len(), 4 * SQUARE_SIDE_SEGMENTS);
        assert!(points.iter().all(|p| (square.distance(*p) - 5.0).abs() < 1e-4));
    }

    #[test]
    fn a_draped_ring_follows_the_ground_and_skips_holes() {
        let footprint = Footprint { center: Vec2::ZERO, radius: 4.0, square: false };
        let slope = |p: Vec2| Some(p.x * 0.5);
        let segments = drape_loop(&footprint.outline(1.0), slope);
        assert_eq!(segments.len(), RING_SEGMENTS, "a closed loop has as many segments as points");
        for [a, b] in &segments {
            assert!((a.y - (a.x * 0.5 + GROUND_LIFT)).abs() < 1e-4);
            assert!((b.y - (b.x * 0.5 + GROUND_LIFT)).abs() < 1e-4);
        }
        // A hole over x > 0 drops every segment with an end there.
        let holed = |p: Vec2| (p.x <= 0.0).then_some(0.0);
        let segments = drape_loop(&footprint.outline(1.0), holed);
        assert!(!segments.is_empty() && segments.len() < RING_SEGMENTS);
        assert!(segments.iter().all(|[a, b]| a.x <= 1e-4 && b.x <= 1e-4));
    }

    #[test]
    fn half_push_ring_matches_the_falloff() {
        for falloff in [0.15, 0.5, 1.0] {
            let fraction = half_push_fraction(falloff);
            assert!((falloff_weight(fraction, 1.0, falloff) - 0.5).abs() < 1e-3, "falloff {falloff}");
        }
        assert_eq!(half_push_fraction(0.0), 1.0, "a hard brush pushes fully to the rim");
        // Linear falloff halves exactly halfway out.
        assert!((half_push_fraction(1.0) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn strength_disc_opacity_follows_strength_and_falloff() {
        let footprint = Footprint { center: Vec2::ZERO, radius: 8.0, square: false };
        let mesh = strength_disc(footprint, 1.0, 0.5, None, flat(2.0));
        assert_eq!(mesh.positions.len(), 1 + DISC_RINGS * DISC_SEGMENTS);
        assert_eq!(mesh.indices.len(), 3 * (DISC_SEGMENTS + 2 * DISC_SEGMENTS * (DISC_RINGS - 1)));
        assert!((mesh.alphas[0] - DISC_MAX_ALPHA).abs() < 1e-6, "full push at the centre");
        assert!(mesh.alphas.last().unwrap().abs() < 1e-6, "no push at the rim");
        assert!(mesh.positions.iter().all(|p| (p.y - (2.0 + GROUND_LIFT)).abs() < 1e-6));
        let half = strength_disc(footprint, 0.5, 0.5, None, flat(2.0));
        assert!((half.alphas[0] - DISC_MAX_ALPHA * 0.5).abs() < 1e-6, "half strength, half opacity");
        // On a locked plane the disc lies on the plane, not the ground.
        let planed = strength_disc(footprint, 1.0, 0.5, Some(10.0), flat(2.0));
        assert!(planed.positions.iter().all(|p| (p.y - (10.0 + GROUND_LIFT)).abs() < 1e-6));
        // Over no ground nothing is drawn.
        let none = strength_disc(footprint, 1.0, 0.5, None, |_| None);
        assert!(none.positions.is_empty() && none.indices.is_empty());
    }

    #[test]
    fn wireframes_trace_the_shape_and_clip_to_the_plane() {
        let sphere = CsgShape::Sphere { center: Vec3::new(0.0, 5.0, 0.0), radius: 2.0 };
        let wire = volume_wireframe(&sphere, None);
        assert_eq!(wire.len(), 3 * WIRE_CIRCLE_SEGMENTS);
        assert!(wire.iter().flatten().all(|p| (p.distance(Vec3::new(0.0, 5.0, 0.0)) - 2.0).abs() < 1e-4));

        let cube = CsgShape::AxisBox { center: Vec3::ZERO, half_extents: Vec3::new(1.0, 2.0, 3.0) };
        let edges = volume_wireframe(&cube, None);
        assert_eq!(edges.len(), 12);
        assert!(edges.iter().flatten().all(|p| p.x.abs() == 1.0 && p.y.abs() == 2.0 && p.z.abs() == 3.0));

        let cylinder = CsgShape::Cylinder { center: Vec3::ZERO, radius: 1.0, half_height: 2.0 };
        assert_eq!(volume_wireframe(&cylinder, None).len(), 2 * WIRE_CIRCLE_SEGMENTS + 4);

        // Clipped at y = 0 keeping below: nothing above the plane survives,
        // and the box's upright edges are cut at it.
        let clipped = volume_wireframe(&cube, Some((0.0, true)));
        assert!(clipped.iter().flatten().all(|p| p.y <= 1e-6));
        assert_eq!(clipped.len(), 8, "four bottom edges and four upright edges cut in half");
    }

    #[test]
    fn grid_lines_sit_on_the_snap_lattice_and_fade_out() {
        let segments = local_grid(Vec2::new(0.3, 0.3), 5.0, 1.0, None, 0.4, 0.8, flat(0.0));
        assert!(!segments.is_empty());
        for s in &segments {
            // Every segment runs along a line of constant x or z at a whole metre.
            let on_x = (s.a.x - s.a.x.round()).abs() < 1e-4 && (s.b.x - s.a.x).abs() < 1e-4;
            let on_z = (s.a.z - s.a.z.round()).abs() < 1e-4 && (s.b.z - s.a.z).abs() < 1e-4;
            assert!(on_x || on_z, "{s:?}");
            assert!(s.alpha_a <= 0.8 + 1e-6 && s.alpha_a >= 0.0);
        }
        // The line through the origin is a major line (every tenth).
        assert!(segments.iter().any(|s| s.a.x.abs() < 1e-4 && s.alpha_a > 0.4));
        // A wide patch over a fine step steps the grid up.
        assert_eq!(grid_step_for(100.0, 0.25), 2.5);
        assert!(local_grid(Vec2::ZERO, 100.0, 0.25, None, 0.4, 0.8, flat(0.0)).len() < 20_000);
    }

    #[test]
    fn contours_cross_a_slope_at_every_interval() {
        // Ground rising 1 m per metre along x, over a disc of radius 4:
        // levels -4..=4 m, so nine iso-lines of constant x.
        let slope = |p: Vec2| Some(p.x);
        let segments = contours(Vec2::ZERO, 4.0, 1.0, 0.3, 0.6, slope);
        assert!(!segments.is_empty());
        for s in &segments {
            assert!((s.a.y - GROUND_LIFT - s.a.x).abs() < 1e-3, "a contour at height h runs where the ground is h");
            assert!((s.a.x - s.b.x).abs() < 1e-3, "on this slope every contour is a line of constant x");
            assert!(((s.a.y - GROUND_LIFT) - (s.a.y - GROUND_LIFT).round()).abs() < 1e-3);
        }
        // Flat ground has no contour.
        assert!(contours(Vec2::ZERO, 4.0, 1.0, 0.3, 0.6, flat(3.0)).is_empty());
        assert_eq!(contour_interval_for(1000.0, 1.0), 50.0);
    }

    #[test]
    fn the_locked_plane_is_flat_and_gridded() {
        let (fill, rim, grid) = plane_disc(Vec2::new(1.0, 1.0), 7.5, 6.0, 1.0, 0.12);
        assert_eq!(fill.positions.len(), 1 + RING_SEGMENTS);
        assert!(fill.positions.iter().all(|p| p.y == 7.5));
        assert_eq!(rim.len(), RING_SEGMENTS);
        assert!(!grid.is_empty());
        assert!(grid.iter().flatten().all(|p| (p.y - 7.5).abs() < 1e-5));
    }

    #[test]
    fn mirror_lines_run_along_the_mirror_planes() {
        let lines = mirror_lines(Vec2::new(5.0, 5.0), Vec2::new(2.0, -1.0), 10.0, true, true, flat(0.0));
        assert!(lines.iter().any(|[a, b]| (a.x - 2.0).abs() < 1e-5 && (b.x - 2.0).abs() < 1e-5));
        assert!(lines.iter().any(|[a, b]| (a.z + 1.0).abs() < 1e-5 && (b.z + 1.0).abs() < 1e-5));
        assert!(mirror_lines(Vec2::ZERO, Vec2::ZERO, 10.0, false, false, flat(0.0)).is_empty());
    }
}

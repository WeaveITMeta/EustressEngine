//! Road design: the geometry a road is built to, before any mesh or terrain.
//!
//! Pure math on `f64`, no ECS and no engine types, so the road tool, the
//! layer bake, the road surface and the bridge's `road.audit` all evaluate
//! one design. See `docs/design/ROAD_SYSTEM.md`.
//!
//! - [`RoadClass`] and [`RoadStandard`]: the numbers a class is designed to
//!   (lanes, widths, design speed, maximum grade, minimum radius, vertical
//!   curve K, superelevation, cut and fill slopes, structure limits).
//! - [`Alignment`]: the horizontal alignment through points of intersection
//!   (PIs). Each interior PI gets a circular arc entered and left through
//!   clothoid spirals, at the largest radius the neighbouring curves leave
//!   room for and never under the standard's minimum unless nothing else
//!   fits (reported as a violation).
//! - [`VerticalProfile`]: grades between vertical points of intersection
//!   joined by parabolic vertical curves. [`VerticalProfile::auto`] fits one
//!   to the ground under the alignment within the standard's grade and K.
//! - [`CrossSection`]: the formation at a station (lanes with crown or
//!   superelevation, shoulders, kerb and sidewalk or ditch) and the daylight
//!   slope from its edge to the ground.
//! - [`audit`]: every place the design breaks its standard.
//!
//! Coordinates: `x` east, `z` south (the engine's world XZ), elevations in
//! metres, stations `s` in metres along the alignment, headings in radians
//! from +X toward +Z. Seen from above with north (-Z) up, +Z is south, so a
//! heading turning toward +Z turns right: positive curvature is a right turn,
//! and positive offsets lie on the right of the direction of travel.

use std::f64::consts::PI;

// ============================================================================
// Vectors
// ============================================================================

/// A point or direction in the horizontal plane.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct P2 {
    pub x: f64,
    pub z: f64,
}

impl P2 {
    pub const fn new(x: f64, z: f64) -> Self {
        Self { x, z }
    }
    fn add(self, o: P2) -> P2 {
        P2::new(self.x + o.x, self.z + o.z)
    }
    fn sub(self, o: P2) -> P2 {
        P2::new(self.x - o.x, self.z - o.z)
    }
    fn scale(self, k: f64) -> P2 {
        P2::new(self.x * k, self.z * k)
    }
    fn length(self) -> f64 {
        self.x.hypot(self.z)
    }
    fn dir(heading: f64) -> P2 {
        P2::new(heading.cos(), heading.sin())
    }
    /// The point `offset` metres across `heading` from this one, positive to the right of travel.
    pub fn offset(self, heading: f64, offset: f64) -> P2 {
        self.add(P2::normal(heading).scale(offset))
    }
    fn heading(self) -> f64 {
        self.z.atan2(self.x)
    }
    /// The unit normal on the positive-offset side of `heading` (the right of travel).
    pub fn normal(heading: f64) -> P2 {
        P2::new(-heading.sin(), heading.cos())
    }
}

/// `a - b` wrapped to `(-PI, PI]`.
fn angle_diff(a: f64, b: f64) -> f64 {
    let mut d = (a - b) % (2.0 * PI);
    if d > PI {
        d -= 2.0 * PI;
    } else if d <= -PI {
        d += 2.0 * PI;
    }
    d
}

// ============================================================================
// Classes and standards
// ============================================================================

/// What a road is for, which decides the standard it is designed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoadClass {
    Motorway,
    Arterial,
    Collector,
    Local,
    Mountain,
    Rural,
    Unpaved,
    Driveway,
}

impl RoadClass {
    pub const ALL: [RoadClass; 8] = [
        RoadClass::Motorway,
        RoadClass::Arterial,
        RoadClass::Collector,
        RoadClass::Local,
        RoadClass::Mountain,
        RoadClass::Rural,
        RoadClass::Unpaved,
        RoadClass::Driveway,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RoadClass::Motorway => "Motorway",
            RoadClass::Arterial => "Arterial",
            RoadClass::Collector => "Collector",
            RoadClass::Local => "Local",
            RoadClass::Mountain => "Mountain",
            RoadClass::Rural => "Rural",
            RoadClass::Unpaved => "Unpaved",
            RoadClass::Driveway => "Driveway",
        }
    }

    pub fn from_name(name: &str) -> Option<RoadClass> {
        RoadClass::ALL
            .into_iter()
            .find(|c| c.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// Where a designed road's elevation comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProfileMode {
    /// Graded to the ground under the alignment within the class's grade and
    /// vertical curves ([`VerticalProfile::auto`]).
    #[default]
    Auto,
    /// Through the heights of the road's points, joined by vertical curves.
    Explicit,
    /// Following the ground, as a track does.
    Ground,
}

impl ProfileMode {
    pub const ALL: [ProfileMode; 3] = [ProfileMode::Auto, ProfileMode::Explicit, ProfileMode::Ground];

    pub fn name(self) -> &'static str {
        match self {
            ProfileMode::Auto => "Auto",
            ProfileMode::Explicit => "Explicit",
            ProfileMode::Ground => "Ground",
        }
    }

    pub fn from_name(name: &str) -> Option<ProfileMode> {
        ProfileMode::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// How a road's edge is chosen: its class's own, or one set on the road.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EdgeChoice {
    /// The class's edge (a Local street's kerb and sidewalk, a rural road's ditch).
    #[default]
    Auto,
    Sidewalk,
    Ditch,
    None,
}

impl EdgeChoice {
    pub const ALL: [EdgeChoice; 4] = [EdgeChoice::Auto, EdgeChoice::Sidewalk, EdgeChoice::Ditch, EdgeChoice::None];

    pub fn name(self) -> &'static str {
        match self {
            EdgeChoice::Auto => "Auto",
            EdgeChoice::Sidewalk => "Sidewalk",
            EdgeChoice::Ditch => "Ditch",
            EdgeChoice::None => "None",
        }
    }

    pub fn from_name(name: &str) -> Option<EdgeChoice> {
        EdgeChoice::ALL.into_iter().find(|e| e.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// Most lanes a designed road takes in one direction.
pub const MAX_LANES: u8 = 4;
/// Width of a parking lane along a kerb, metres.
pub const PARKING_LANE: f64 = 2.4;
/// Width of the gutter between the lanes and a kerb, metres.
pub const GUTTER: f64 = 0.5;

/// What a road spline is designed to: its class, where its profile comes
/// from, and how its lanes are laid out. A road without one is laid as
/// before the road system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoadSpec {
    pub class: RoadClass,
    pub profile: ProfileMode,
    /// Lanes in each direction, or every lane of a one-way road; 0 keeps the class's.
    pub lanes: u8,
    /// Every lane runs the road's own way (toward its last point).
    pub one_way: bool,
    /// A parking lane along each kerb.
    pub parking: bool,
    pub edge: EdgeChoice,
}

impl RoadSpec {
    /// A road of `class` with its profile from `profile`, laid out as the class is.
    pub fn new(class: RoadClass, profile: ProfileMode) -> Self {
        Self { class, profile, lanes: 0, one_way: false, parking: false, edge: EdgeChoice::Auto }
    }

    /// The class's standard with this road's lanes, one-way, parking and
    /// edge, its carriageway set to `width` metres when `width` is positive
    /// (every lane shares it, each kept between 2.5 and 4.5 m), so a
    /// spline's Width still sizes the road.
    pub fn standard(&self, width: f64) -> RoadStandard {
        let mut s = RoadStandard::of(self.class);
        if self.lanes > 0 {
            s.lanes_per_direction = u32::from(self.lanes.min(MAX_LANES));
        }
        s.one_way = self.one_way;
        if self.one_way {
            // One way has no median to divide.
            s.median_width = 0.0;
        }
        match self.edge {
            EdgeChoice::Auto => {}
            EdgeChoice::Sidewalk => {
                if s.edge != EdgeTreatment::KerbAndSidewalk {
                    s.edge = EdgeTreatment::KerbAndSidewalk;
                    s.sidewalk_width = if self.class == RoadClass::Local { 2.0 } else { 3.0 };
                    s.kerb_height = 0.15;
                    // A kerbed street drains to a gutter, not a shoulder.
                    s.shoulder_width = GUTTER;
                    s.shoulder_paved = true;
                }
            }
            EdgeChoice::Ditch => s.edge = EdgeTreatment::Ditch,
            EdgeChoice::None => s.edge = EdgeTreatment::None,
        }
        if self.parking {
            s.parking = true;
            s.shoulder_width = PARKING_LANE;
            s.shoulder_paved = true;
        }
        if width.is_finite() && width > 0.0 {
            let lanes = s.lanes_total() as f64;
            s.lane_width = ((width - s.median_width).max(0.0) / lanes).clamp(2.5, 4.5);
        }
        s
    }
}

/// How a road's edge is finished beyond its shoulder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeTreatment {
    /// A kerb and a raised sidewalk (towns).
    KerbAndSidewalk,
    /// An open drainage ditch at the foot of a cut, or the toe of a fill.
    Ditch,
    /// The shoulder runs straight into the daylight slope.
    None,
}

/// The numbers a road class is designed to. Every value is a default a
/// Space or a single road may override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadStandard {
    pub class: RoadClass,
    /// Lanes in each direction (every lane, when `one_way`).
    pub lanes_per_direction: u32,
    /// Every lane runs the road's own way: toward increasing distance along it.
    pub one_way: bool,
    /// The shoulder is a parking lane (striped with stalls).
    pub parking: bool,
    pub lane_width: f64,
    /// Width of the central median (0 for an undivided road).
    pub median_width: f64,
    pub shoulder_width: f64,
    pub shoulder_paved: bool,
    pub edge: EdgeTreatment,
    pub sidewalk_width: f64,
    pub kerb_height: f64,
    /// Design speed, km/h.
    pub design_speed: f64,
    /// Steepest sustained grade (rise over run, 0.08 = 8%).
    pub max_grade: f64,
    /// Steepest grade allowed over a short length (hairpins, ramps).
    pub max_grade_short: f64,
    /// Smallest horizontal radius at the design speed, metres.
    pub min_radius: f64,
    /// Smallest radius at a hairpin (Mountain), metres.
    pub hairpin_radius: f64,
    /// Vertical curve length per percent of grade change, crest and sag.
    pub k_crest: f64,
    pub k_sag: f64,
    /// Normal crown, the cross slope each way from the centre.
    pub crown: f64,
    /// Steepest superelevation through a curve.
    pub max_superelevation: f64,
    /// Daylight slopes as horizontal over vertical: cut in soil, cut in rock, fill.
    pub cut_slope: f64,
    pub rock_cut_slope: f64,
    pub fill_slope: f64,
    /// Fill deeper than this becomes a bridge, cut deeper a tunnel, metres.
    pub max_fill: f64,
    pub max_cut: f64,
}

impl RoadStandard {
    /// Distance from the centreline to the outer edge of the formation on
    /// either side: half the median, the lanes, the shoulder and the edge
    /// (kerb and sidewalk, or ditch). [`CrossSection::formation`] ends there.
    pub fn formation_half_width(&self) -> f64 {
        let edge = match self.edge {
            EdgeTreatment::KerbAndSidewalk => 0.15 + self.sidewalk_width,
            EdgeTreatment::Ditch => 3.0,
            EdgeTreatment::None => 0.0,
        };
        self.median_width / 2.0 + self.half_lanes_width() + self.shoulder_width + edge
    }

    /// Every lane the road has, both ways.
    pub fn lanes_total(&self) -> u32 {
        if self.one_way {
            self.lanes_per_direction
        } else {
            2 * self.lanes_per_direction
        }
    }

    /// Width of the lanes on each side of the centreline: a two-way road's
    /// lanes in one direction, half a one-way road's.
    pub fn half_lanes_width(&self) -> f64 {
        self.lanes_total() as f64 * self.lane_width / 2.0
    }

    /// The default standard of `class`.
    pub fn of(class: RoadClass) -> RoadStandard {
        use EdgeTreatment::*;
        use RoadClass::*;
        let base = RoadStandard {
            class,
            lanes_per_direction: 1,
            one_way: false,
            parking: false,
            lane_width: 3.5,
            median_width: 0.0,
            shoulder_width: 1.5,
            shoulder_paved: true,
            edge: None,
            sidewalk_width: 0.0,
            kerb_height: 0.0,
            design_speed: 50.0,
            max_grade: 0.08,
            max_grade_short: 0.10,
            min_radius: 90.0,
            hairpin_radius: 0.0,
            k_crest: 7.0,
            k_sag: 13.0,
            crown: 0.02,
            max_superelevation: 0.06,
            cut_slope: 1.5,
            rock_cut_slope: 0.5,
            fill_slope: 2.0,
            max_fill: 12.0,
            max_cut: 18.0,
        };
        match class {
            Motorway => RoadStandard {
                lanes_per_direction: 2,
                lane_width: 3.65,
                median_width: 4.0,
                shoulder_width: 3.0,
                design_speed: 110.0,
                max_grade: 0.04,
                max_grade_short: 0.05,
                min_radius: 600.0,
                k_crest: 74.0,
                k_sag: 49.0,
                max_superelevation: 0.08,
                max_fill: 15.0,
                max_cut: 20.0,
                ..base
            },
            Arterial => RoadStandard {
                lanes_per_direction: 2,
                shoulder_width: 2.0,
                design_speed: 70.0,
                max_grade: 0.06,
                max_grade_short: 0.08,
                min_radius: 200.0,
                k_crest: 17.0,
                k_sag: 23.0,
                ..base
            },
            Collector => base,
            Local => RoadStandard {
                lane_width: 3.0,
                shoulder_width: 0.5,
                edge: KerbAndSidewalk,
                sidewalk_width: 2.0,
                kerb_height: 0.15,
                design_speed: 40.0,
                max_grade: 0.10,
                max_grade_short: 0.12,
                min_radius: 50.0,
                k_crest: 4.0,
                k_sag: 9.0,
                max_superelevation: 0.04,
                max_fill: 8.0,
                max_cut: 10.0,
                ..base
            },
            Mountain => RoadStandard {
                lane_width: 3.25,
                shoulder_width: 0.75,
                shoulder_paved: false,
                edge: Ditch,
                design_speed: 40.0,
                max_grade: 0.10,
                max_grade_short: 0.12,
                min_radius: 50.0,
                hairpin_radius: 30.0,
                k_crest: 4.0,
                k_sag: 9.0,
                crown: 0.025,
                max_superelevation: 0.08,
                max_fill: 10.0,
                max_cut: 16.0,
                ..base
            },
            Rural => RoadStandard {
                lane_width: 3.25,
                shoulder_width: 1.0,
                shoulder_paved: false,
                edge: Ditch,
                design_speed: 70.0,
                max_grade: 0.08,
                max_grade_short: 0.10,
                min_radius: 150.0,
                k_crest: 17.0,
                k_sag: 23.0,
                ..base
            },
            Unpaved => RoadStandard {
                lane_width: 3.0,
                shoulder_width: 0.0,
                shoulder_paved: false,
                design_speed: 30.0,
                max_grade: 0.12,
                max_grade_short: 0.15,
                min_radius: 25.0,
                k_crest: 2.0,
                k_sag: 5.0,
                crown: 0.04,
                max_superelevation: 0.0,
                max_fill: 6.0,
                max_cut: 8.0,
                ..base
            },
            Driveway => RoadStandard {
                lane_width: 3.0,
                shoulder_width: 0.0,
                design_speed: 20.0,
                max_grade: 0.15,
                max_grade_short: 0.18,
                min_radius: 10.0,
                k_crest: 1.0,
                k_sag: 2.0,
                max_superelevation: 0.0,
                max_fill: 3.0,
                max_cut: 3.0,
                ..base
            },
        }
    }

    /// Width of the carriageway (every lane and the median), metres.
    pub fn carriageway_width(&self) -> f64 {
        self.lanes_total() as f64 * self.lane_width + self.median_width
    }

    /// Width of the formation (carriageway, shoulders, sidewalks), metres.
    pub fn formation_width(&self) -> f64 {
        self.carriageway_width() + 2.0 * (self.shoulder_width + self.sidewalk_width)
    }

    /// Spiral length for a curve of radius `r` at the design speed: the
    /// rate of change of lateral acceleration held to 0.6 m/s3 (a comfortable
    /// value), and never under the length the superelevation runs over.
    pub fn spiral_length(&self, r: f64) -> f64 {
        if !(r.is_finite() && r > 0.0) {
            return 0.0;
        }
        let v = self.design_speed / 3.6;
        let comfort = v * v * v / (r * 0.6);
        let runoff = self.superelevation(r) * self.carriageway_width() * 0.5 / 0.007;
        comfort.max(runoff).max(10.0).min(r * 1.2)
    }

    /// Superelevation for a curve of radius `r` (AASHTO's method 5 curve,
    /// simplified): rising from the crown on gentle curves to the maximum at
    /// the minimum radius.
    pub fn superelevation(&self, r: f64) -> f64 {
        if !(r.is_finite() && r > 0.0) || self.max_superelevation <= 0.0 {
            return 0.0;
        }
        let ratio = (self.min_radius / r).min(1.0);
        (self.max_superelevation * (2.0 * ratio - ratio * ratio)).max(if ratio > 0.25 {
            self.crown
        } else {
            0.0
        })
    }
}

// ============================================================================
// Horizontal alignment
// ============================================================================

/// One element of an alignment, in order along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    /// A straight run.
    Tangent { length: f64 },
    /// A clothoid whose curvature changes linearly from `k0` to `k1` (signed:
    /// positive turns right, toward +heading).
    Spiral { length: f64, k0: f64, k1: f64 },
    /// A circular arc of constant signed curvature.
    Arc { length: f64, k: f64 },
}

impl Element {
    pub fn length(&self) -> f64 {
        match *self {
            Element::Tangent { length }
            | Element::Spiral { length, .. }
            | Element::Arc { length, .. } => length,
        }
    }
    /// Curvature `t` metres into the element.
    fn curvature(&self, t: f64) -> f64 {
        match *self {
            Element::Tangent { .. } => 0.0,
            Element::Arc { k, .. } => k,
            Element::Spiral { length, k0, k1 } => k0 + (k1 - k0) * (t / length.max(1e-9)),
        }
    }
}

/// The curve fitted at one interior PI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveFit {
    /// Index of the PI in the alignment's points.
    pub pi: usize,
    /// Deflection angle, signed (positive turns right), radians.
    pub deflection: f64,
    pub radius: f64,
    pub spiral_length: f64,
    /// Distance from the PI back to the curve's start and on to its end.
    pub tangent_length: f64,
    /// Whether the radius had to go under the standard's minimum to fit.
    pub below_minimum: bool,
}

/// A station along an alignment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub s: f64,
    pub pos: P2,
    pub heading: f64,
    /// Signed curvature (1 / radius, positive turning right).
    pub curvature: f64,
}

/// The horizontal alignment of a road: elements from a start point and
/// heading, fitted through PIs.
#[derive(Clone, Debug, PartialEq)]
pub struct Alignment {
    pub start: P2,
    pub start_heading: f64,
    pub elements: Vec<Element>,
    pub curves: Vec<CurveFit>,
    pub length: f64,
}

/// Spiral offsets for a spiral of length `ls` into radius `r`: the throw
/// `p` and the tangent shift `k` (series, accurate for spiral angles under
/// about 0.8 rad).
fn spiral_shift(r: f64, ls: f64) -> (f64, f64) {
    if ls <= 0.0 {
        return (0.0, 0.0);
    }
    let th = ls / (2.0 * r);
    let xs = ls * (1.0 - th * th / 10.0 + th.powi(4) / 216.0);
    let ys = ls * (th / 3.0 - th.powi(3) / 42.0 + th.powi(5) / 1320.0);
    (ys - r * (1.0 - th.cos()), xs - r * th.sin())
}

/// Tangent length of a spiral-arc-spiral curve of radius `r`, spiral `ls`,
/// deflection `delta` (unsigned). The spiral shortens when the deflection
/// is too small for two full spirals.
fn curve_tangent(r: f64, ls: f64, delta: f64) -> (f64, f64) {
    let ls = ls.min(r * delta).max(0.0); // two spirals of angle ls/2r each never exceed delta
    let (p, k) = spiral_shift(r, ls);
    ((r + p) * (delta / 2.0).tan() + k, ls)
}

/// Deflection over which a curve on a road with a hairpin radius is part of
/// a switchback: a hairpin is usually laid as two or three such curves.
const HAIRPIN_DEFLECTION_DEG: f64 = 75.0;

/// Whether a curve of deflection `delta` (unsigned) is a hairpin of `standard`.
fn is_hairpin(standard: &RoadStandard, delta: f64) -> bool {
    standard.hairpin_radius > 0.0 && delta > HAIRPIN_DEFLECTION_DEG.to_radians()
}

/// Smallest radius `standard` allows for a curve of deflection `delta`.
fn curve_floor(standard: &RoadStandard, delta: f64) -> f64 {
    if is_hairpin(standard, delta) {
        standard.hairpin_radius
    } else {
        standard.min_radius
    }
}

/// Largest value in `lo..=hi` for which `ok` holds, `ok(lo)` being true and
/// `ok` monotone (true below some value, false above), by bisection.
fn largest_where(lo: f64, hi: f64, ok: impl Fn(f64) -> bool) -> f64 {
    if ok(hi) {
        return hi;
    }
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if ok(mid) {
            lo = mid
        } else {
            hi = mid
        }
    }
    lo
}

impl Alignment {
    /// Fit an alignment through `points` (at least two) to `standard`,
    /// preferring `preferred_radius` at every curve (or twice the standard's
    /// minimum when `None`), and using the hairpin radius for the turns of a
    /// switchback (deflections over [`HAIRPIN_DEFLECTION_DEG`]) when the
    /// standard has one. A leg too short for the curves at its ends costs
    /// their spirals first and their radii only once the spirals are gone.
    pub fn fit(
        points: &[P2],
        standard: &RoadStandard,
        preferred_radius: Option<f64>,
    ) -> Option<Alignment> {
        if points.len() < 2 || points.iter().any(|p| !(p.x.is_finite() && p.z.is_finite())) {
            return None;
        }
        let n = points.len();
        let legs: Vec<P2> = (0..n - 1).map(|i| points[i + 1].sub(points[i])).collect();
        if legs.iter().any(|l| l.length() < 1e-6) {
            return None;
        }
        // Deflections at interior PIs.
        let mut fits: Vec<CurveFit> = (1..n - 1)
            .map(|i| {
                let deflection = angle_diff(legs[i].heading(), legs[i - 1].heading());
                let delta = deflection.abs();
                let r = if is_hairpin(standard, delta) {
                    standard.hairpin_radius
                } else {
                    preferred_radius
                        .unwrap_or(standard.min_radius * 2.0)
                        .max(standard.min_radius)
                };
                let (t, ls) = curve_tangent(r, standard.spiral_length(r), delta);
                CurveFit {
                    pi: i,
                    deflection,
                    radius: r,
                    spiral_length: ls,
                    tangent_length: t,
                    below_minimum: false,
                }
            })
            .collect();
        // Shrink radii until every leg holds the tangents of the curves at its ends.
        for _ in 0..60 {
            let mut changed = false;
            for leg in 0..n - 1 {
                let before = if leg >= 1 {
                    fits[leg - 1].tangent_length
                } else {
                    0.0
                };
                let after = if leg + 1 <= n - 2 {
                    fits[leg].tangent_length
                } else {
                    0.0
                };
                let available = legs[leg].length();
                if before + after <= available + 1e-9 {
                    continue;
                }
                changed = true;
                let scale = available / (before + after);
                for (idx, share) in [(leg.wrapping_sub(1), before), (leg, after)] {
                    if idx >= fits.len() || share <= 0.0 {
                        continue;
                    }
                    let f = &mut fits[idx];
                    let delta = f.deflection.abs();
                    let target = share * scale;
                    let floor = curve_floor(standard, delta).min(f.radius);
                    // The radius is the safety value and the spirals are
                    // comfort, so a short leg costs spiral before radius:
                    // first the largest radius down to the floor with full
                    // spirals, then the floor with shorter spirals, and only
                    // with no spiral left a radius under the floor.
                    let (radius, spiral) = if curve_tangent(floor, standard.spiral_length(floor), delta).0 <= target {
                        let r = largest_where(floor, f.radius, |r| {
                            curve_tangent(r, standard.spiral_length(r), delta).0 <= target
                        });
                        (r, standard.spiral_length(r))
                    } else if curve_tangent(floor, 0.0, delta).0 <= target {
                        let full = standard.spiral_length(floor);
                        let ls = largest_where(0.0, full, |ls| curve_tangent(floor, ls, delta).0 <= target);
                        (floor, ls)
                    } else {
                        let r = largest_where(0.5f64.min(floor), floor, |r| curve_tangent(r, 0.0, delta).0 <= target);
                        (r, 0.0)
                    };
                    let (t, ls) = curve_tangent(radius, spiral, delta);
                    *f = CurveFit {
                        radius,
                        spiral_length: ls,
                        tangent_length: t,
                        below_minimum: radius + 1e-6 < curve_floor(standard, delta),
                        ..*f
                    };
                }
            }
            if !changed {
                break;
            }
        }
        // Elements: tangent, then spiral-arc-spiral per curve.
        let mut elements = Vec::new();
        let mut prev_end = points[0];
        for f in &fits {
            let pi = points[f.pi];
            let in_dir = legs[f.pi - 1].scale(1.0 / legs[f.pi - 1].length());
            let ts = pi.sub(in_dir.scale(f.tangent_length));
            let tangent = ts.sub(prev_end).length();
            if tangent > 1e-6 {
                elements.push(Element::Tangent { length: tangent });
            }
            let sign = f.deflection.signum();
            let k = sign / f.radius;
            let delta = f.deflection.abs();
            let arc = (f.radius * (delta - f.spiral_length / f.radius)).max(0.0);
            if f.spiral_length > 0.0 {
                elements.push(Element::Spiral {
                    length: f.spiral_length,
                    k0: 0.0,
                    k1: k,
                });
            }
            if arc > 1e-9 {
                elements.push(Element::Arc { length: arc, k });
            }
            if f.spiral_length > 0.0 {
                elements.push(Element::Spiral {
                    length: f.spiral_length,
                    k0: k,
                    k1: 0.0,
                });
            }
            let out_dir = legs[f.pi].scale(1.0 / legs[f.pi].length());
            prev_end = pi.add(out_dir.scale(f.tangent_length));
        }
        let last = points[n - 1].sub(prev_end).length();
        if last > 1e-6 {
            elements.push(Element::Tangent { length: last });
        }
        let length = elements.iter().map(Element::length).sum();
        Some(Alignment {
            start: points[0],
            start_heading: legs[0].heading(),
            elements,
            curves: fits,
            length,
        })
    }

    /// Fit an alignment that passes through `points`: the ends are the road's
    /// ends and every interior point is the middle of its curve, which is how
    /// a person draws a road (a hairpin's PI lies far beyond the turn). Each
    /// PI is moved out along its curve's bisector by the curve's external
    /// distance until the curve's middle lands on its point.
    pub fn fit_through(
        points: &[P2],
        standard: &RoadStandard,
        preferred_radius: Option<f64>,
    ) -> Option<Alignment> {
        let n = points.len();
        if n < 3 {
            return Alignment::fit(points, standard, preferred_radius);
        }
        let mut pis = points.to_vec();
        let mut fitted = Alignment::fit(&pis, standard, preferred_radius)?;
        for _ in 0..40 {
            let mut worst = 0.0f64;
            for c in &fitted.curves {
                let i = c.pi;
                let dir_in = pis[i].sub(pis[i - 1]);
                let dir_out = pis[i + 1].sub(pis[i]);
                let (a, b) = (
                    dir_in.scale(1.0 / dir_in.length()),
                    dir_out.scale(1.0 / dir_out.length()),
                );
                let inside = b.sub(a);
                if inside.length() < 1e-9 {
                    continue;
                }
                let inside = inside.scale(1.0 / inside.length());
                let (p, _) = spiral_shift(c.radius, c.spiral_length);
                let external = (c.radius + p) / (c.deflection.abs() / 2.0).cos() - c.radius;
                let target = points[i].sub(inside.scale(external));
                worst = worst.max(target.sub(pis[i]).length());
                pis[i] = target;
            }
            fitted = Alignment::fit(&pis, standard, preferred_radius)?;
            if worst < 0.01 {
                break;
            }
        }
        Some(fitted)
    }

    /// Stations every `step` metres from start to end (the end included),
    /// integrated through each element's curvature.
    pub fn stations(&self, step: f64) -> Vec<Station> {
        let step = step.max(0.05);
        let sub = 0.25f64.min(step); // integration step
        let mut out = Vec::new();
        let (mut pos, mut heading, mut s) = (self.start, self.start_heading, 0.0f64);
        let mut next_emit = 0.0f64;
        out.push(Station {
            s: 0.0,
            pos,
            heading,
            curvature: self.elements.first().map_or(0.0, |e| e.curvature(0.0)),
        });
        next_emit += step;
        for e in &self.elements {
            let len = e.length();
            let mut t = 0.0;
            while t < len - 1e-12 {
                let h = sub.min(len - t);
                // Midpoint rule on heading: exact for arcs, second order for spirals.
                let k0 = e.curvature(t);
                let km = e.curvature(t + h * 0.5);
                let k1 = e.curvature(t + h);
                let mid_heading = heading + (k0 + km) * 0.25 * h;
                pos = pos.add(P2::dir(mid_heading).scale(h));
                heading += (k0 + 4.0 * km + k1) / 6.0 * h;
                t += h;
                s += h;
                if s + 1e-9 >= next_emit {
                    out.push(Station {
                        s,
                        pos,
                        heading,
                        curvature: k1,
                    });
                    next_emit += step;
                }
            }
        }
        if out.last().is_some_and(|st| (st.s - s).abs() > 1e-6) {
            out.push(Station {
                s,
                pos,
                heading,
                curvature: 0.0,
            });
        }
        out
    }
}

// ============================================================================
// Vertical profile
// ============================================================================

/// A vertical point of intersection: the profile's grade changes at station
/// `s`, through elevation `z`, eased by a parabolic curve `curve_length`
/// long centred on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vpi {
    pub s: f64,
    pub elevation: f64,
    pub curve_length: f64,
}

/// A road's elevation along its stations.
#[derive(Clone, Debug, PartialEq)]
pub struct VerticalProfile {
    pub vpis: Vec<Vpi>,
}

impl VerticalProfile {
    /// The profile through `vpis` (stations increasing), each vertical curve
    /// set from the standard's K for its grade change and shrunk so no two
    /// curves overlap and none runs past the ends.
    pub fn from_vpis(mut vpis: Vec<Vpi>, standard: &RoadStandard) -> VerticalProfile {
        vpis.sort_by(|a, b| a.s.total_cmp(&b.s));
        let n = vpis.len();
        for i in 1..n.saturating_sub(1) {
            let g1 =
                (vpis[i].elevation - vpis[i - 1].elevation) / (vpis[i].s - vpis[i - 1].s).max(1e-9);
            let g2 =
                (vpis[i + 1].elevation - vpis[i].elevation) / (vpis[i + 1].s - vpis[i].s).max(1e-9);
            let a = (g2 - g1) * 100.0; // percent
            let k = if a < 0.0 {
                standard.k_crest
            } else {
                standard.k_sag
            };
            vpis[i].curve_length = k * a.abs();
        }
        if n > 0 {
            vpis[0].curve_length = 0.0;
            vpis[n - 1].curve_length = 0.0;
        }
        // No overlaps: two half-curves share their gap in proportion.
        for i in 0..n.saturating_sub(1) {
            let gap = vpis[i + 1].s - vpis[i].s;
            let need = 0.5 * (vpis[i].curve_length + vpis[i + 1].curve_length);
            if need > gap && need > 0.0 {
                let k = gap / need;
                vpis[i].curve_length *= k;
                vpis[i + 1].curve_length *= k;
            }
        }
        VerticalProfile { vpis }
    }

    /// The profile that passes through every `(station, elevation)` point,
    /// the vertical twin of [`Alignment::fit_through`]: a vertical curve
    /// passes below a crest's VPI and above a sag's, so each VPI is raised or
    /// lowered until its curve's middle lands on its point.
    pub fn through(points: &[(f64, f64)], standard: &RoadStandard) -> VerticalProfile {
        let mut pts = points.to_vec();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let make = |pts: &[(f64, f64)]| {
            VerticalProfile::from_vpis(
                pts.iter()
                    .map(|&(s, elevation)| Vpi {
                        s,
                        elevation,
                        curve_length: 0.0,
                    })
                    .collect(),
                standard,
            )
        };
        let target = pts.clone();
        let mut profile = make(&pts);
        for _ in 0..60 {
            let mut worst = 0.0f64;
            for i in 1..pts.len().saturating_sub(1) {
                let miss = target[i].1 - profile.elevation(target[i].0);
                worst = worst.max(miss.abs());
                pts[i].1 += miss;
            }
            if worst < 0.005 {
                break;
            }
            profile = make(&pts);
        }
        profile
    }

    /// Elevation at station `s` (held flat past the ends).
    pub fn elevation(&self, s: f64) -> f64 {
        let v = &self.vpis;
        match v.len() {
            0 => 0.0,
            1 => v[0].elevation,
            _ => {
                let s = s.clamp(v[0].s, v[v.len() - 1].s);
                let grade = |i: usize| {
                    (v[i + 1].elevation - v[i].elevation) / (v[i + 1].s - v[i].s).max(1e-9)
                };
                // On a vertical curve?
                for i in 1..v.len() - 1 {
                    let l = v[i].curve_length;
                    if l > 0.0 && (s - v[i].s).abs() <= l / 2.0 {
                        let (g1, g2) = (grade(i - 1), grade(i));
                        let bvc = v[i].s - l / 2.0;
                        let z_bvc = v[i].elevation - g1 * l / 2.0;
                        let x = s - bvc;
                        return z_bvc + g1 * x + (g2 - g1) / (2.0 * l) * x * x;
                    }
                }
                let i = v
                    .windows(2)
                    .position(|w| s <= w[1].s)
                    .unwrap_or(v.len() - 2);
                v[i].elevation + grade(i) * (s - v[i].s)
            }
        }
    }

    /// Grade (rise over run) at station `s`.
    pub fn grade(&self, s: f64) -> f64 {
        let h = 0.5;
        (self.elevation(s + h) - self.elevation(s - h)) / (2.0 * h)
    }

    /// Fit a profile to `ground` (elevations at stations `step` apart from
    /// station 0): the profile nearest the ground whose grade never exceeds
    /// the standard's maximum, simplified to VPIs at most `tolerance` from
    /// it, then eased with vertical curves. Each end starts on the ground and
    /// leaves it only where the ground there is steeper than the grade allows.
    pub fn auto(
        ground: &[f64],
        step: f64,
        standard: &RoadStandard,
        tolerance: f64,
    ) -> VerticalProfile {
        let n = ground.len();
        if n == 0 {
            return VerticalProfile { vpis: Vec::new() };
        }
        // Smooth the ground over about 40 m: a road never follows every bump.
        let half = ((20.0 / step).round() as usize).max(1);
        let mut prof: Vec<f64> = (0..n)
            .map(|i| {
                let (a, b) = (i.saturating_sub(half), (i + half).min(n - 1));
                ground[a..=b].iter().sum::<f64>() / (b - a + 1) as f64
            })
            .collect();
        prof[0] = ground[0];
        prof[n - 1] = ground[n - 1];
        // Hold the grade: forward and backward envelopes until stable.
        let g = standard.max_grade * step;
        for _ in 0..8 {
            let mut moved = false;
            for i in 1..n {
                let clamped = prof[i].clamp(prof[i - 1] - g, prof[i - 1] + g);
                if (clamped - prof[i]).abs() > 1e-9 {
                    prof[i] = clamped;
                    moved = true;
                }
            }
            for i in (0..n - 1).rev() {
                let clamped = prof[i].clamp(prof[i + 1] - g, prof[i + 1] + g);
                if (clamped - prof[i]).abs() > 1e-9 {
                    prof[i] = clamped;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        // Simplify to VPIs (Ramer-Douglas-Peucker on station, elevation).
        let pts: Vec<(f64, f64)> = prof
            .iter()
            .enumerate()
            .map(|(i, &z)| (i as f64 * step, z))
            .collect();
        let mut keep = vec![false; n];
        keep[0] = true;
        keep[n - 1] = true;
        fn rdp(p: &[(f64, f64)], lo: usize, hi: usize, tol: f64, keep: &mut [bool]) {
            if hi <= lo + 1 {
                return;
            }
            let (a, b) = (p[lo], p[hi]);
            let (mut best, mut idx) = (0.0f64, lo);
            for i in lo + 1..hi {
                let t = (p[i].0 - a.0) / (b.0 - a.0);
                let d = (p[i].1 - (a.1 + (b.1 - a.1) * t)).abs();
                if d > best {
                    best = d;
                    idx = i;
                }
            }
            if best > tol {
                keep[idx] = true;
                rdp(p, lo, idx, tol, keep);
                rdp(p, idx, hi, tol, keep);
            }
        }
        rdp(&pts, 0, n - 1, tolerance.max(0.05), &mut keep);
        let vpis = pts
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(&(s, z), _)| Vpi {
                s,
                elevation: z,
                curve_length: 0.0,
            })
            .collect();
        VerticalProfile::from_vpis(vpis, standard)
    }
}

// ============================================================================
// Cross-section
// ============================================================================

/// What a stretch of the cross-section is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Lane,
    Median,
    Shoulder,
    Kerb,
    Sidewalk,
    Ditch,
    CutSlope,
    FillSlope,
}

/// One break point of a cross-section: `offset` metres from the centreline
/// (positive to the right of the direction of travel), at `elevation`
/// relative to the profile, starting a stretch of `zone` that runs to the
/// next point outward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPoint {
    pub offset: f64,
    pub elevation: f64,
    pub zone: Zone,
}

/// The formation at one station, its two halves from the centre outward.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossSection {
    /// The positive-offset half (the right of travel), from the centre out.
    pub positive: Vec<SectionPoint>,
    /// The negative-offset half (the left of travel), from the centre out.
    pub negative: Vec<SectionPoint>,
    /// Cross slope applied (positive raises the positive-offset edge).
    pub cross_slope: f64,
}

/// Cross slope at a station of curvature `k` (signed): the crown on
/// tangents, rising to the curve's superelevation along the spiral as the
/// curvature does, banked toward the inside of the curve.
pub fn cross_slope(standard: &RoadStandard, k: f64, full_curve_k: f64) -> f64 {
    if k.abs() < 1e-9 || full_curve_k.abs() < 1e-9 {
        return 0.0; // crown handled per side
    }
    let e = standard.superelevation(1.0 / full_curve_k.abs());
    let ramp = (k.abs() / full_curve_k.abs()).min(1.0);
    // Banked toward the inside: a right turn (k > 0) lowers the right (positive) edge.
    -k.signum() * e * ramp
}

impl CrossSection {
    /// Elevation over the profile at `offset` (positive to the right of
    /// travel): linear between its points, level past its last.
    pub fn elevation_at(&self, offset: f64) -> f64 {
        let half = if offset >= 0.0 { &self.positive } else { &self.negative };
        let o = offset.abs();
        let mut prev: Option<&SectionPoint> = None;
        for p in half {
            let po = p.offset.abs();
            if o <= po {
                return match prev {
                    Some(q) => {
                        let qo = q.offset.abs();
                        q.elevation + (p.elevation - q.elevation) * ((o - qo) / (po - qo).max(1e-12))
                    }
                    None => p.elevation,
                };
            }
            prev = Some(p);
        }
        prev.map_or(0.0, |p| p.elevation)
    }

    /// The formation of `standard` with cross slope `slope` (0 for the
    /// normal crown; otherwise one plane across the carriageway).
    pub fn formation(standard: &RoadStandard, slope: f64) -> CrossSection {
        let side = |sign: f64| {
            let mut pts = Vec::new();
            let mut off = 0.0f64;
            // The carriageway slopes down from the centre (crown) or along one plane (superelevation).
            let lane_slope = |o: f64| {
                if slope == 0.0 {
                    -standard.crown * o
                } else {
                    slope * sign * o
                }
            };
            if standard.median_width > 0.0 {
                pts.push(SectionPoint {
                    offset: 0.0,
                    elevation: 0.0,
                    zone: Zone::Median,
                });
                off = standard.median_width / 2.0;
            }
            pts.push(SectionPoint {
                offset: sign * off,
                elevation: lane_slope(off),
                zone: Zone::Lane,
            });
            off += standard.half_lanes_width();
            let shoulder_slope = (standard.crown * 2.0).max(0.04);
            let lane_edge = lane_slope(off);
            pts.push(SectionPoint {
                offset: sign * off,
                elevation: lane_edge,
                zone: Zone::Shoulder,
            });
            off += standard.shoulder_width;
            let mut z = lane_edge - shoulder_slope * standard.shoulder_width;
            match standard.edge {
                EdgeTreatment::KerbAndSidewalk => {
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::Kerb,
                    });
                    z += standard.kerb_height;
                    pts.push(SectionPoint {
                        offset: sign * (off + 0.15),
                        elevation: z,
                        zone: Zone::Sidewalk,
                    });
                    off += 0.15 + standard.sidewalk_width;
                    z += 0.02 * standard.sidewalk_width;
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::CutSlope,
                    });
                }
                EdgeTreatment::Ditch => {
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::Ditch,
                    });
                    // A V ditch 0.6 m deep, 1:3 in and 1:2 out.
                    off += 1.8;
                    z -= 0.6;
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::Ditch,
                    });
                    off += 1.2;
                    z += 0.6;
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::CutSlope,
                    });
                }
                EdgeTreatment::None => {
                    pts.push(SectionPoint {
                        offset: sign * off,
                        elevation: z,
                        zone: Zone::CutSlope,
                    });
                }
            }
            pts
        };
        CrossSection {
            positive: side(1.0),
            negative: side(-1.0),
            cross_slope: slope,
        }
    }

    /// Where the daylight slope from this side's outer edge meets the ground.
    /// `ground(offset)` is the ground elevation at an offset, relative to the
    /// profile; `rock` picks the rock cut slope. Returns the catch point
    /// (offset, relative elevation) and whether it is in cut, and replaces
    /// the side's last zone accordingly.
    pub fn daylight(
        &mut self,
        positive_side: bool,
        standard: &RoadStandard,
        rock: bool,
        ground: impl Fn(f64) -> f64,
    ) -> (f64, f64, bool) {
        let side = if positive_side {
            &mut self.positive
        } else {
            &mut self.negative
        };
        let edge = *side.last().expect("a side always has an outer edge");
        let sign = if positive_side { 1.0 } else { -1.0 };
        let g = ground(edge.offset);
        let cut = g > edge.elevation;
        let slope = if cut {
            if rock {
                standard.rock_cut_slope
            } else {
                standard.cut_slope
            }
        } else {
            standard.fill_slope
        };
        // Walk out along the slope until it crosses the ground (step 0.25 m, then refine).
        let at = |d: f64| edge.elevation + if cut { d / slope } else { -d / slope };
        let (mut d0, mut d1) = (0.0f64, 0.0f64);
        let mut found = false;
        while d1 < 400.0 {
            d1 += 0.25;
            let diff = at(d1) - ground(edge.offset + sign * d1);
            if (cut && diff >= 0.0) || (!cut && diff <= 0.0) {
                found = true;
                break;
            }
            d0 = d1;
        }
        if found {
            for _ in 0..30 {
                let m = 0.5 * (d0 + d1);
                let diff = at(m) - ground(edge.offset + sign * m);
                if (cut && diff >= 0.0) || (!cut && diff <= 0.0) {
                    d1 = m
                } else {
                    d0 = m
                }
            }
        }
        let last = side.len() - 1;
        side[last].zone = if cut { Zone::CutSlope } else { Zone::FillSlope };
        let catch = (edge.offset + sign * d1, at(d1));
        side.push(SectionPoint {
            offset: catch.0,
            elevation: catch.1,
            zone: if cut { Zone::CutSlope } else { Zone::FillSlope },
        });
        (catch.0, catch.1, cut)
    }
}

// ============================================================================
// Audit
// ============================================================================

/// One way a design breaks its standard.
#[derive(Clone, Debug, PartialEq)]
pub enum Violation {
    /// A curve's radius is under the standard's minimum (or hairpin radius).
    Radius {
        pi: usize,
        radius: f64,
        minimum: f64,
    },
    /// The profile is steeper than allowed between two stations.
    Grade {
        from: f64,
        to: f64,
        grade: f64,
        maximum: f64,
    },
    /// The ground is further below the profile than a fill may be.
    Fill {
        from: f64,
        to: f64,
        depth: f64,
        maximum: f64,
    },
    /// The ground is further above the profile than a cut may be.
    Cut {
        from: f64,
        to: f64,
        depth: f64,
        maximum: f64,
    },
}

/// Every violation of `standard` by an alignment and its profile over
/// `ground` (elevations at the alignment's stations `step` apart). Runs of
/// consecutive offending stations are reported once.
pub fn audit(
    alignment: &Alignment,
    profile: &VerticalProfile,
    ground: &[f64],
    step: f64,
    standard: &RoadStandard,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for c in &alignment.curves {
        if c.below_minimum {
            out.push(Violation::Radius {
                pi: c.pi,
                radius: c.radius,
                minimum: if is_hairpin(standard, c.deflection.abs()) {
                    standard.hairpin_radius
                } else {
                    standard.min_radius
                },
            });
        }
    }
    let mut run = |test: &dyn Fn(f64, f64) -> Option<(f64, f64)>,
                   make: &dyn Fn(f64, f64, f64) -> Violation| {
        let mut start: Option<(f64, f64)> = None;
        for (i, &g) in ground.iter().enumerate() {
            let s = i as f64 * step;
            match (test(s, g), start) {
                (Some((value, _)), None) => start = Some((s, value)),
                (Some((value, _)), Some((a, worst))) => start = Some((a, worst.max(value))),
                (None, Some((a, worst))) => {
                    out.push(make(a, s - step, worst));
                    start = None;
                }
                (None, None) => {}
            }
        }
        if let Some((a, worst)) = start {
            out.push(make(a, (ground.len().max(1) - 1) as f64 * step, worst));
        }
    };
    let max_g = standard.max_grade_short;
    run(
        &|s, _| {
            let g = profile.grade(s).abs();
            (g > max_g + 1e-6).then_some((g, max_g))
        },
        &|a, b, w| Violation::Grade {
            from: a,
            to: b,
            grade: w,
            maximum: max_g,
        },
    );
    run(
        &|s, g| {
            let d = profile.elevation(s) - g;
            (d > standard.max_fill).then_some((d, standard.max_fill))
        },
        &|a, b, w| Violation::Fill {
            from: a,
            to: b,
            depth: w,
            maximum: standard.max_fill,
        },
    );
    run(
        &|s, g| {
            let d = g - profile.elevation(s);
            (d > standard.max_cut).then_some((d, standard.max_cut))
        },
        &|a, b, w| Violation::Cut {
            from: a,
            to: b,
            depth: w,
            maximum: standard.max_cut,
        },
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn standards_widths_follow_their_lanes() {
        let m = RoadStandard::of(RoadClass::Motorway);
        assert!(close(m.carriageway_width(), 2.0 * 2.0 * 3.65 + 4.0, 1e-9));
        let l = RoadStandard::of(RoadClass::Local);
        assert!(close(l.formation_width(), 6.0 + 2.0 * (0.5 + 2.0), 1e-9));
        assert!(RoadStandard::of(RoadClass::Mountain).hairpin_radius > 0.0);
        // The formation's half width is where its cross-section ends.
        for class in RoadClass::ALL {
            let s = RoadStandard::of(class);
            let x = CrossSection::formation(&s, 0.0);
            let edge = x.positive.last().unwrap().offset;
            assert!(close(s.formation_half_width(), edge, 1e-9), "{}", class.name());
            assert!(close(-x.negative.last().unwrap().offset, edge, 1e-9));
            assert_eq!(RoadClass::from_name(&class.name().to_lowercase()), Some(class));
        }
        assert_eq!(RoadClass::from_name("None"), None);
        for mode in ProfileMode::ALL {
            assert_eq!(ProfileMode::from_name(mode.name()), Some(mode));
        }
    }

    #[test]
    fn superelevation_reaches_the_maximum_at_the_minimum_radius() {
        let a = RoadStandard::of(RoadClass::Arterial);
        assert!(close(
            a.superelevation(a.min_radius),
            a.max_superelevation,
            1e-9
        ));
        assert!(a.superelevation(a.min_radius * 4.0) < a.max_superelevation);
        assert!(a.superelevation(1e9) < 1e-6);
    }

    #[test]
    fn a_straight_alignment_is_one_tangent() {
        let std = RoadStandard::of(RoadClass::Collector);
        let a = Alignment::fit(&[P2::new(0.0, 0.0), P2::new(300.0, 400.0)], &std, None).unwrap();
        assert_eq!(a.elements.len(), 1);
        assert!(close(a.length, 500.0, 1e-9));
        let st = a.stations(10.0);
        let end = st.last().unwrap();
        assert!(close(end.pos.x, 300.0, 1e-6) && close(end.pos.z, 400.0, 1e-6));
    }

    #[test]
    fn a_curve_leaves_on_the_outgoing_tangent_exactly() {
        // A 90 degree turn at (500, 0) heading +X, then +Z.
        let std = RoadStandard::of(RoadClass::Collector);
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(500.0, 0.0),
            P2::new(500.0, 500.0),
        ];
        let a = Alignment::fit(&pts, &std, Some(150.0)).unwrap();
        let c = a.curves[0];
        assert!(
            close(c.radius, 150.0, 1e-9),
            "the preferred radius fits: {}",
            c.radius
        );
        assert!(c.spiral_length > 0.0);
        let st = a.stations(1.0);
        let end = st.last().unwrap();
        assert!(
            close(end.pos.x, 500.0, 0.05) && close(end.pos.z, 500.0, 0.05),
            "ends on the last point: {:?}",
            end.pos
        );
        assert!(
            close(angle_diff(end.heading, PI / 2.0), 0.0, 1e-3),
            "leaves heading +Z: {}",
            end.heading
        );
        // Curvature never jumps: spirals ease it in and out.
        let max_jump = st
            .windows(2)
            .map(|w| (w[1].curvature - w[0].curvature).abs())
            .fold(0.0, f64::max);
        assert!(
            max_jump < 1.0 / 150.0 * 0.2,
            "curvature jumps by {max_jump}"
        );
        // The tightest curvature is the arc's.
        let kmax = st.iter().map(|s| s.curvature.abs()).fold(0.0, f64::max);
        assert!(close(kmax, 1.0 / 150.0, 1e-6));
    }

    #[test]
    fn a_short_leg_costs_spiral_before_radius() {
        // A switchback: two 90 degree turns whose shared leg holds two 30 m
        // hairpin arcs with no room for their full spirals.
        let std = RoadStandard::of(RoadClass::Mountain);
        let r = std.hairpin_radius;
        let leg = 2.0 * r + 4.0; // two tangents of r at 90 degrees, plus a little
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(300.0, 0.0),
            P2::new(300.0, leg),
            P2::new(0.0, leg),
        ];
        let a = Alignment::fit(&pts, &std, None).unwrap();
        for c in &a.curves {
            assert!(close(c.radius, r, 1e-6), "the hairpin radius is kept: {}", c.radius);
            assert!(!c.below_minimum);
            assert!(c.spiral_length < std.spiral_length(r), "the spiral gave way: {}", c.spiral_length);
        }
        let used: f64 = a.curves.iter().map(|c| c.tangent_length).sum();
        assert!(used <= leg + 1e-6, "both tangents fit the {leg} m leg: {used}");
        // Shorter than two bare arcs: only then does the radius go under.
        let tight = [pts[0], pts[1], P2::new(300.0, r), P2::new(0.0, r)];
        let a = Alignment::fit(&tight, &std, None).unwrap();
        assert!(a.curves.iter().all(|c| c.spiral_length == 0.0 && c.radius < r && c.below_minimum));
    }

    #[test]
    fn curves_shrink_to_fit_a_short_leg_and_report_it() {
        let std = RoadStandard::of(RoadClass::Arterial);
        // Two 90 degree turns 60 m apart: no room for 200 m radii.
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(400.0, 0.0),
            P2::new(400.0, 60.0),
            P2::new(800.0, 60.0),
        ];
        let a = Alignment::fit(&pts, &std, None).unwrap();
        let t1 = a.curves[0].tangent_length;
        let t2 = a.curves[1].tangent_length;
        assert!(
            t1 + t2 <= 60.0 + 1e-6,
            "tangents {t1} + {t2} fit the 60 m leg"
        );
        assert!(a.curves.iter().all(|c| c.below_minimum));
        let v = audit(&a, &VerticalProfile { vpis: vec![] }, &[], 1.0, &std);
        assert_eq!(
            v.iter()
                .filter(|v| matches!(v, Violation::Radius { .. }))
                .count(),
            2
        );
        let st = a.stations(1.0);
        let end = st.last().unwrap();
        assert!(
            close(end.pos.x, 800.0, 0.1) && close(end.pos.z, 60.0, 0.1),
            "{:?}",
            end.pos
        );
    }

    #[test]
    fn a_road_drawn_through_points_passes_through_them() {
        let std = RoadStandard::of(RoadClass::Mountain);
        // Switchbacks drawn as the points the road should pass: the hairpins pass through them.
        let pts = [
            P2::new(0.0, 0.0),
            P2::new(300.0, 70.0),
            P2::new(20.0, 140.0),
            P2::new(300.0, 210.0),
        ];
        let a = Alignment::fit_through(&pts, &std, None).unwrap();
        let st = a.stations(0.5);
        for p in &pts {
            let d = st
                .iter()
                .map(|s| (s.pos.x - p.x).hypot(s.pos.z - p.z))
                .fold(f64::INFINITY, f64::min);
            assert!(d < 0.5, "the road passes {d} m from {p:?}");
        }
    }

    #[test]
    fn a_mountain_hairpin_uses_the_hairpin_radius() {
        let std = RoadStandard::of(RoadClass::Mountain);
        let pts = [P2::new(0.0, 0.0), P2::new(200.0, 0.0), P2::new(0.0, 40.0)];
        let a = Alignment::fit(&pts, &std, None).unwrap();
        assert!(a.curves[0].deflection.abs() > 120f64.to_radians());
        assert!(a.curves[0].radius <= std.hairpin_radius + 1e-9);
    }

    #[test]
    fn vertical_curves_join_grades_smoothly() {
        let std = RoadStandard::of(RoadClass::Collector);
        let p = VerticalProfile::from_vpis(
            vec![
                Vpi {
                    s: 0.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
                Vpi {
                    s: 200.0,
                    elevation: 10.0,
                    curve_length: 0.0,
                },
                Vpi {
                    s: 400.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
            ],
            &std,
        );
        let l = p.vpis[1].curve_length;
        assert!(close(l, std.k_crest * 10.0, 1e-9), "K x A = {}", l);
        // Continuous elevation and grade through the curve's ends.
        for s in [200.0 - l / 2.0, 200.0 + l / 2.0] {
            assert!(close(p.elevation(s - 1e-4), p.elevation(s + 1e-4), 1e-3));
            assert!(close(p.grade(s - 0.6), p.grade(s + 0.6), 0.02));
        }
        assert!(
            p.elevation(200.0) < 10.0,
            "the crest is rounded below the VPI"
        );
        assert!(close(p.grade(50.0), 0.05, 1e-9) && close(p.grade(350.0), -0.05, 1e-9));
    }

    #[test]
    fn the_auto_profile_holds_the_grade_and_stays_near_the_ground() {
        let std = RoadStandard::of(RoadClass::Mountain);
        // Ground: a 30% ramp with bumps, then flat.
        let ground: Vec<f64> = (0..=400)
            .map(|i| {
                let s = i as f64;
                (if s < 200.0 { 0.3 * s } else { 60.0 }) + 2.0 * (s * 0.2).sin()
            })
            .collect();
        let p = VerticalProfile::auto(&ground, 1.0, &std, 0.3);
        for s in (0..400).map(|i| i as f64 + 0.5) {
            assert!(
                p.grade(s).abs() <= std.max_grade + 0.005,
                "grade {} at {s}",
                p.grade(s)
            );
        }
        assert!(close(p.elevation(0.0), ground[0], 1e-6));
        // Too steep for the class: the audit reports the cut or fill it needs.
        let a = Alignment::fit(&[P2::new(0.0, 0.0), P2::new(400.0, 0.0)], &std, None).unwrap();
        let v = audit(&a, &p, &ground, 1.0, &std);
        assert!(
            v.iter()
                .any(|v| matches!(v, Violation::Fill { .. } | Violation::Cut { .. })),
            "{v:?}"
        );
    }

    #[test]
    fn the_formation_crowns_and_banks() {
        let std = RoadStandard::of(RoadClass::Collector);
        let crown = CrossSection::formation(&std, 0.0);
        let lane_edge = crown
            .positive
            .iter()
            .find(|p| p.zone == Zone::Shoulder)
            .unwrap();
        assert!(close(lane_edge.offset, 3.5, 1e-9) && close(lane_edge.elevation, -0.07, 1e-9));
        let r_edge = crown
            .negative
            .iter()
            .find(|p| p.zone == Zone::Shoulder)
            .unwrap();
        assert!(close(r_edge.offset, -3.5, 1e-9) && close(r_edge.elevation, -0.07, 1e-9));
        // Superelevated: a right turn lowers the right (positive) edge.
        let e = cross_slope(&std, 1.0 / 100.0, 1.0 / 100.0);
        assert!(e < 0.0);
        let banked = CrossSection::formation(&std, e);
        let l = banked
            .positive
            .iter()
            .find(|p| p.zone == Zone::Shoulder)
            .unwrap();
        let r = banked
            .negative
            .iter()
            .find(|p| p.zone == Zone::Shoulder)
            .unwrap();
        assert!(l.elevation < 0.0 && r.elevation > 0.0);
    }

    #[test]
    fn daylight_slopes_meet_the_ground() {
        let std = RoadStandard::of(RoadClass::Rural);
        // Ground rising 1:4 to the positive side, 3 m above the profile at the centre: a cut there.
        let mut x = CrossSection::formation(&std, 0.0);
        let (off, z, cut) = x.daylight(true, &std, false, |o| 3.0 + o / 4.0);
        assert!(cut);
        assert!(
            close(z, 3.0 + off / 4.0, 0.01),
            "catch point on the ground: {off}, {z}"
        );
        // Ground 5 m below everywhere on the negative side: a fill at 1:2.
        let (off_r, z_r, cut_r) = x.daylight(false, &std, false, |_| -5.0);
        assert!(!cut_r && close(z_r, -5.0, 0.01));
        let edge = x.negative[x.negative.len() - 2];
        assert!(close(
            (edge.offset - off_r).abs(),
            (edge.elevation + 5.0) * std.fill_slope,
            0.02
        ));
    }
}

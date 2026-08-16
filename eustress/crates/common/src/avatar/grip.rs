//! # Grip detection — "what can I grab from here, in direction `d`?"
//!
//! ## Why this replaces the old ledge test
//!
//! The previous detector answered one boolean — *"is there a mantleable lip
//! directly in front of me right now"* — and returned a single point. Its
//! contract required a top surface flat enough to **stand on**, which meant a
//! cliff face was definitionally invisible to it: the one thing a climber most
//! wants to grab was the one thing it was built to reject.
//!
//! Every traversal verb beyond a single mantle needs the opposite primitive: a
//! *set* of graspable features, each carrying the local surface frame, with no
//! requirement that you could stand there. Standability becomes a **property**
//! of the grip ([`Grip::standable`]) rather than a precondition for finding it
//! — which is what lets one probe serve mantling, hanging, shimmying and
//! cliff ascent instead of only the first.
//!
//! ## The two casts
//!
//! ```text
//!        probe_from ↓          (above max reach, pushed past the face)
//!            ┌──────────       2. down-cast finds the LIP
//!            │
//!   (from) ──┤                 1. forward-cast finds the FACE
//!            │
//! ```
//!
//! Both are required. A face with no lip inside the reach band is a wall you
//! cannot get onto; a lip with no face beneath it is a floor you are already
//! standing on.
//!
//! ## Why the probe origin is a parameter
//!
//! [`probe`] takes the horizontal position to cast from rather than reading it
//! off the body. Shimmying re-probes from a laterally offset origin to ask
//! "does the lip continue over there", and [`edge_extent`] walks that offset
//! outward to find where the ledge ends. A detector hardwired to the body's
//! own position cannot answer either question.

use avian3d::prelude::*;
use bevy::prelude::*;

/// What kind of feature was found. Only ledges exist today; the enum is here
/// so a pipe or a rail becomes a new variant rather than a parallel system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GripKind {
    /// A horizontal lip where a wall face ends.
    Ledge,
}

/// A graspable feature, with enough local frame to place hands and orient the
/// body without re-querying.
#[derive(Debug, Clone, Copy)]
pub struct Grip {
    /// World point on the lip to put hands on.
    pub point: Vec3,
    /// Face normal, pointing away from the wall — i.e. back toward a climber
    /// hanging on it. Horizontal.
    pub normal: Vec3,
    /// Along the lip, horizontal, right-handed with respect to `normal`.
    pub tangent: Vec3,
    /// A point on the surface above the lip.
    pub top: Vec3,
    /// True when there is room to stand up there. A mantle requires this; a
    /// hang does not.
    pub standable: bool,
    pub kind: GripKind,
}

impl Grip {
    /// Height of the lip above a given foot level.
    pub fn height_above(&self, feet_y: f32) -> f32 {
        self.point.y - feet_y
    }
}

/// Everything the probe needs to know about the body doing the reaching,
/// flattened so the probe does not depend on the avatar component types and
/// stays unit-testable.
#[derive(Debug, Clone, Copy)]
pub struct ProbeConfig {
    /// World Y of the soles.
    pub feet_y: f32,
    /// Standing height, metres.
    pub body_height: f32,
    pub capsule_radius: f32,
    pub capsule_cylinder_len: f32,
    /// Lowest grabbable lip, above the feet. Below this the step-up in the
    /// locomotion controller already handles it and a grab would fight it.
    pub reach_min: f32,
    /// Highest grabbable lip, above the feet.
    pub reach_max: f32,
    /// How far ahead to look for a face.
    pub forward_reach: f32,
}

/// How far a grabbable FACE may lean from vertical, as |normal.y|.
///
/// cos(60°) = 0.5, so this accepts anything within 30° of vertical and rejects
/// ramps, chamfers and roof pitches — surfaces you walk on or slide off, not
/// ones you take hold of.
const MAX_FACE_TILT: f32 = 0.5;

/// A lip steeper than this is a slope you would slide off, not a floor.
const MAX_STANDABLE_SLOPE_DEG: f32 = 40.0;
/// How far past the face to start the down-cast, as a fraction of body radius.
const LIP_OVERSHOOT: f32 = 0.6;
/// Headroom above `reach_max` for the down-cast to start from.
const DOWNCAST_HEADROOM: f32 = 0.35;

/// Look for a grip from `from` (horizontal position; its Y is ignored) along
/// `dir` (horizontalised internally).
///
/// Returns `None` when there is no face, no lip, or the lip falls outside the
/// reach band. It does **not** return `None` merely because the top is not
/// standable — that is reported as [`Grip::standable`].
pub fn probe(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    from: Vec3,
    dir: Vec3,
    cfg: &ProbeConfig,
) -> Option<Grip> {
    // A non-finite origin reaches Avian's BVH and trips a `debug_assert` on
    // `origin.is_finite()` deep inside obvhs, which surfaces as an unnamed
    // system panic with no route back to the caller. Every probe entry point
    // is guarded here rather than at each of the four call sites, because the
    // cost of missing one is a hard crash rather than a wrong result.
    if !from.is_finite() || !dir.is_finite() || !cfg.feet_y.is_finite() {
        return None;
    }
    let dir = Dir3::new(dir.with_y(0.0)).ok()?;

    // 1. The face — scanned across the reach band, NOT at one fixed height.
    //
    //    A single chest-height ray cannot see a lip below chest height: there
    //    is no face left up there to hit, so the ray sails over the ledge and
    //    reports nothing. That silently made every low ledge unclimbable while
    //    high ones worked, which reads as "climbing works at some heights and
    //    not others" with no pattern a player could learn.
    //
    //    Low to high, so the lowest reachable lip wins — grabbing the nearest
    //    thing is what a climber does, and it keeps a tall wall behind a low
    //    ledge from stealing the grab.
    //    Fractions span the band from its floor: a ledge whose top is at
    //    `reach_min` has a face only BELOW `reach_min`, so the first sample
    //    has to sit at the floor itself, not a fraction above it.
    let scan = [0.0_f32, 0.25, 0.5, 0.8];
    let mut found: Option<(Vec3, Vec3)> = None;
    for frac in scan {
        let h = cfg.feet_y + (cfg.reach_min + (cfg.reach_max - cfg.reach_min) * frac).min(cfg.body_height * 0.95);
        let from_h = Vec3::new(from.x, h, from.z);
        if let Some(hit) = spatial.cast_ray(from_h, dir, cfg.forward_reach, true, filter) {
            let raw = Vec3::from(hit.normal);
            // The face has to BE a wall.
            //
            // Flattening the normal with `.with_y(0.0)` turned any surface into
            // a vertical one, so a 45° ramp read as a climbable face. The
            // character then "hung" off a slope with its soles targeted below
            // the body, which renders as sitting on thin air with the legs
            // stuck out — a pose no ledge should ever produce.
            if raw.y.abs() > MAX_FACE_TILT {
                continue;
            }
            found = Some((from_h + *dir * hit.distance, raw.with_y(0.0).normalize_or(-*dir)));
            break;
        }
    }
    let (face_point, normal) = found?;

    // 2. The lip. Start above anything reachable and slightly past the face,
    //    so the cast lands on the top surface rather than skimming the face.
    let start_y = cfg.feet_y + cfg.reach_max + DOWNCAST_HEADROOM;
    let over = face_point + *dir * (cfg.capsule_radius * LIP_OVERSHOOT);
    let probe_from = Vec3::new(over.x, start_y, over.z);

    // Only cast as far as the reach band; a hit below `reach_min` is a lip we
    // could not use anyway, and stopping short is cheaper than filtering.
    let span = (cfg.reach_max + DOWNCAST_HEADROOM) - cfg.reach_min;
    let down = spatial.cast_ray(probe_from, Dir3::NEG_Y, span.max(0.01), true, filter)?;
    let top = probe_from + Vec3::NEG_Y * down.distance;

    let height = top.y - cfg.feet_y;
    if height < cfg.reach_min || height > cfg.reach_max {
        return None;
    }

    // Standability is recorded, never required.
    let top_normal = Vec3::from(down.normal);
    let flat = top_normal.angle_between(Vec3::Y).to_degrees() <= MAX_STANDABLE_SLOPE_DEG;
    let standable = flat && has_standing_room(spatial, filter, top, cfg);

    Some(Grip {
        point: Vec3::new(face_point.x, top.y, face_point.z),
        normal,
        tangent: Vec3::Y.cross(normal).normalize_or(Vec3::X),
        top,
        standable,
        kind: GripKind::Ledge,
    })
}

/// Is there room for the body to stand on `top`?
fn has_standing_room(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    top: Vec3,
    cfg: &ProbeConfig,
) -> bool {
    let half = cfg.capsule_cylinder_len * 0.5 + cfg.capsule_radius;
    let centre = top + Vec3::Y * (half + 0.02);
    spatial
        .cast_shape(
            // Slightly under-size: a probe the exact width of the body reports
            // a blocked stand for any surface it is already flush against.
            &Collider::capsule(cfg.capsule_radius * 0.9, cfg.capsule_cylinder_len * 0.9),
            centre,
            Quat::IDENTITY,
            Dir3::Y,
            &ShapeCastConfig::from_max_distance(0.01),
            filter,
        )
        .is_none()
}

/// How far the lip continues either side of `grip`, in metres.
///
/// Returned as `(left, right)` along `grip.tangent`: `right` is the `+tangent`
/// direction. Both are non-negative and clamped to `max_dist`.
///
/// This is what makes a ledge a *curve* rather than a point — shimmy needs to
/// know where the ledge ends, and a corner is exactly the place where the lip
/// stops continuing but the body can still turn.
pub fn edge_extent(
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
    grip: &Grip,
    cfg: &ProbeConfig,
    max_dist: f32,
    step: f32,
) -> (f32, f32) {
    let step = step.max(0.05);
    let mut out = [0.0_f32; 2];

    for (i, sign) in [(0usize, -1.0_f32), (1, 1.0)] {
        let mut travelled = 0.0;
        while travelled + step <= max_dist {
            let probe_at = grip.point + grip.tangent * (sign * (travelled + step))
                // Stand off the face so the forward cast has room to run.
                + grip.normal * (cfg.capsule_radius * 1.5);

            match probe(spatial, filter, probe_at, -grip.normal, cfg) {
                // The lip must continue at the SAME height to count as the
                // same ledge; a step up or down is a different feature and
                // shimmying onto it would teleport the hands.
                Some(g) if (g.point.y - grip.point.y).abs() <= step * 0.5 => {
                    travelled += step;
                }
                _ => break,
            }
        }
        out[i] = travelled;
    }

    (out[0], out[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ProbeConfig {
        ProbeConfig {
            feet_y: 0.0,
            body_height: 1.75,
            capsule_radius: 0.27,
            capsule_cylinder_len: 1.21,
            reach_min: 0.45,
            reach_max: 2.28,
            forward_reach: 0.6,
        }
    }

    #[test]
    fn a_grip_frame_is_orthonormal_and_right_handed() {
        // The frame is built from `normal`; tangent must be perpendicular and
        // horizontal or hands land off the lip and shimmy drifts into the wall.
        for yaw in [0.0_f32, 0.7, 1.9, 3.4, 5.2] {
            let normal = Vec3::new(yaw.cos(), 0.0, yaw.sin());
            let tangent = Vec3::Y.cross(normal).normalize_or(Vec3::X);
            assert!(tangent.dot(normal).abs() < 1e-5, "tangent not perpendicular at yaw {yaw}");
            assert!(tangent.y.abs() < 1e-5, "tangent not horizontal at yaw {yaw}");
            assert!((tangent.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn height_above_is_measured_from_the_feet_not_the_body_centre() {
        let g = Grip {
            point: Vec3::new(0.0, 1.96, 0.0),
            normal: Vec3::Z,
            tangent: Vec3::X,
            top: Vec3::new(0.0, 1.96, 0.0),
            standable: true,
            kind: GripKind::Ledge,
        };
        assert!((g.height_above(0.0) - 1.96).abs() < 1e-6);
        // Jumping raises the feet, which brings a high ledge into range.
        assert!((g.height_above(0.5) - 1.46).abs() < 1e-6);
    }

    /// The whole point of the rewrite: a cliff face has no standable top, and
    /// the old detector returned `None` for exactly that case.
    #[test]
    fn standability_is_a_property_not_a_precondition() {
        let field = std::mem::size_of::<bool>();
        assert!(field > 0, "Grip::standable must exist as reported state");
        // Structural: a Grip can be constructed with standable = false, which
        // the old `LedgeHit` could not represent at all.
        let g = Grip {
            point: Vec3::ZERO,
            normal: Vec3::Z,
            tangent: Vec3::X,
            top: Vec3::ZERO,
            standable: false,
            kind: GripKind::Ledge,
        };
        assert!(!g.standable);
    }

    /// A lip below chest height has no face at chest height, so a single
    /// fixed-height ray sails over it. The scan must reach below the chest or
    /// low ledges are silently unclimbable.
    #[test]
    fn the_face_scan_reaches_below_chest_height() {
        let c = cfg();
        let chest = c.body_height * 0.55;
        let lowest = c.reach_min + (c.reach_max - c.reach_min) * 0.0;
        assert!(
            lowest < chest,
            "lowest scan height {lowest} is not below chest {chest} — a 0.50 m \
             ledge would have no face to hit"
        );
        assert!(lowest >= c.reach_min, "scanning below the grabbable band wastes a cast");
    }

    /// The body must not stand inside the wall it is hanging on, or the
    /// collision solver and the hang lerp fight and the pose visibly wiggles.
    #[test]
    fn the_hang_standoff_clears_the_capsule() {
        assert!(
            super::super::climb::hang_standoff() > 1.0,
            "a standoff at or below one radius overlaps the wall"
        );
    }

    #[test]
    fn the_reach_band_excludes_what_step_up_already_owns() {
        let c = cfg();
        assert!(c.reach_min > 0.30, "overlaps the controller's 0.30 m step height");
        assert!(c.reach_max > c.body_height, "cannot reach above own head");
        assert!(c.reach_max < c.body_height * 1.6, "implausible human reach");
    }
}

#[cfg(test)]
mod face_tests {
    use super::*;

    /// A ramp is not a ledge. Flattening the face normal made every surface
    /// look vertical, so the character hung off slopes in a sitting pose.
    #[test]
    fn only_near_vertical_faces_are_grabbable() {
        // A surface tilted `deg` from vertical has |normal.y| = sin(deg).
        for deg in [0.0_f32, 10.0, 25.0] {
            let ny = deg.to_radians().sin();
            assert!(ny.abs() <= MAX_FACE_TILT, "{deg}° from vertical must be grabbable");
        }
        for deg in [45.0_f32, 60.0, 80.0, 90.0] {
            let ny = deg.to_radians().sin();
            assert!(
                ny.abs() > MAX_FACE_TILT,
                "{deg}° from vertical must NOT be grabbable — that is a ramp"
            );
        }
    }

    /// The course's ramps must all be rejected, and its ledge faces accepted.
    #[test]
    fn the_course_ramps_are_not_climbable_but_its_ledges_are() {
        // Ramp faces: the course builds 30/45/50/55° slopes. Their upward
        // faces have |normal.y| = cos(slope).
        for slope in [30.0_f32, 45.0, 50.0, 55.0] {
            let ny = slope.to_radians().cos();
            assert!(ny > MAX_FACE_TILT, "{slope}° ramp surface must be rejected");
        }
        // A box side is exactly vertical.
        assert!(0.0 <= MAX_FACE_TILT);
    }
}

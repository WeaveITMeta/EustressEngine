//! What the drawn surface and the physics share: the normal of a sloped
//! surface, where foam shows, and how waves fade out toward a shore. A
//! renderer applies the same rules to the textures that the CPU queries
//! apply to the samples, so what floats feels what is drawn.
//!
//! # Foam
//!
//! Whitecaps form where crests are squeezed toward breaking: where the
//! folding Jacobian J of the horizontal displacement (see
//! [`crate::cascade`]) falls below a threshold. [`foam_threshold`] maps the
//! foam setting in [0, 1] linearly onto thresholds from -1 at 0 to 0.9 at 1,
//! and [`foam_coverage`] ramps from no foam at the threshold to full foam
//! [`FOAM_RAMP`] below it. At setting 1 any crest compressed by a tenth
//! starts to foam; lower settings need deeper compression. A threshold of
//! -1 or below means foam is off: [`foam_coverage`] is 0 there whatever J
//! is, since a steep, choppy storm can fold the surface past J = -1 and the
//! setting promises no foam at 0. A renderer should do the same.
//!
//! # Shore fade
//!
//! The water plane runs on under the land, hidden by the terrain, and the
//! surface sits at sea level plus the waves' displacement. A crest is drawn
//! wherever the plane is, so without a fade one would rise through any land
//! lower than itself (a beach, a low island) and a choppy vertex would slide
//! onto the shore. [`shore_fade`] scales the displacement by a smoothstep of
//! the water depth: 1 from the fade depth down, falling to exactly 0 at the
//! shoreline and staying 0 over land, where the water rests flat at sea
//! level under the ground. [`crate::Ocean::shore_fade_depth`] makes the fade
//! depth 1.5 significant wave heights: over that depth a trough, rarely more
//! than one significant height below the rest level, stays above the bed
//! while the waves die away, instead of stopping at a wall. Calm seas still
//! fade over at least 2 m, a distance the terrain resolves. Buoyancy uses the
//! same fade, so a hull run up on a beach feels only the waves drawn there.
//!
//! Real waves do the opposite near a shore: entering shallow water they
//! slow, shorten, steepen and grow until they break (shoaling). The fade
//! trades that for a surface that never cuts through land; breakers belong
//! to a later splash phase, which will add them on top of it.

/// Foam threshold at setting 0, and the threshold at or below which foam
/// is off.
pub const FOAM_OFF_THRESHOLD: f32 = -1.0;
/// Foam threshold at setting 1: foam where the surface is compressed by a
/// tenth.
pub const FOAM_FULL_THRESHOLD: f32 = 0.9;
/// Width of the foam ramp in J: coverage rises from 0 at the threshold to 1
/// this far below it.
pub const FOAM_RAMP: f32 = 0.3;
/// Smallest shore fade depth (m).
pub const MIN_SHORE_FADE_DEPTH: f32 = 2.0;
/// Shore fade depth in significant wave heights.
pub const SHORE_FADE_WAVE_HEIGHTS: f32 = 1.5;

/// The threshold on the folding Jacobian below which foam forms, for a foam
/// setting in [0, 1]: linear, -1 at 0 (foam off) and 0.9 at 1. The setting is
/// clamped to [0, 1]; a NaN setting is 0.
pub fn foam_threshold(foam_setting: f32) -> f32 {
    let s = if foam_setting.is_nan() { 0.0 } else { foam_setting.clamp(0.0, 1.0) };
    FOAM_OFF_THRESHOLD + (FOAM_FULL_THRESHOLD - FOAM_OFF_THRESHOLD) * s
}

/// Foam coverage in [0, 1] where the folding Jacobian is `jacobian`, for a
/// threshold from [`foam_threshold`]: clamp((threshold - J) / 0.3, 0, 1). 0
/// when the threshold is -1 or below (foam off), and for a NaN Jacobian or
/// threshold.
pub fn foam_coverage(jacobian: f32, threshold: f32) -> f32 {
    if !(threshold > FOAM_OFF_THRESHOLD) {
        return 0.0;
    }
    let coverage = (threshold - jacobian) / FOAM_RAMP;
    if coverage > 0.0 {
        coverage.min(1.0)
    } else {
        0.0
    }
}

/// How much of the waves' displacement survives over water `depth` metres
/// deep (sea level minus the height of the ground there), for a fade depth
/// `fade_depth` (m): the smoothstep 3t^2 - 2t^3 of t = clamp(depth /
/// fade_depth, 0, 1). 0 at and below depth 0 (the shoreline and dry land), 1
/// from the fade depth down. A NaN depth (no ground known there) is deep
/// water, 1; a fade depth that is not positive makes the fade a step at the
/// shoreline.
pub fn shore_fade(depth: f32, fade_depth: f32) -> f32 {
    if depth.is_nan() {
        return 1.0;
    }
    if !(depth > 0.0) {
        return 0.0;
    }
    if !(fade_depth > 0.0) {
        return 1.0;
    }
    let t = (depth / fade_depth).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Unit normal of a height field with slopes `slope_x` = dh/dx and
/// `slope_z` = dh/dz: the unit vector along (-dh/dx, 1, -dh/dz), always
/// pointing up.
pub fn normal_from_slopes(slope_x: f32, slope_z: f32) -> [f32; 3] {
    let (sx, sz) = (slope_x as f64, slope_z as f64);
    let inverse_length = 1.0 / (sx * sx + 1.0 + sz * sz).sqrt();
    [(-sx * inverse_length) as f32, inverse_length as f32, (-sz * inverse_length) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foam_threshold_is_linear_from_off_to_point_nine() {
        assert_eq!(foam_threshold(0.0), -1.0);
        assert_eq!(foam_threshold(1.0), 0.9);
        assert!((foam_threshold(0.5) - -0.05).abs() < 1e-6);
        assert!((foam_threshold(0.25) - -0.525).abs() < 1e-6);
        // Clamped to [0, 1]; NaN is 0.
        assert_eq!(foam_threshold(-3.0), -1.0);
        assert_eq!(foam_threshold(7.0), 0.9);
        assert_eq!(foam_threshold(f32::NAN), -1.0);
    }

    #[test]
    fn foam_ramps_in_below_the_threshold_and_is_off_at_setting_zero() {
        let full = foam_threshold(1.0);
        assert_eq!(foam_coverage(1.2, full), 0.0, "stretched water");
        assert_eq!(foam_coverage(0.9, full), 0.0, "at the threshold");
        assert!((foam_coverage(0.75, full) - 0.5).abs() < 1e-5, "halfway down the ramp");
        assert!((foam_coverage(0.6, full) - 1.0).abs() < 1e-5, "the bottom of the ramp");
        assert_eq!(foam_coverage(-4.0, full), 1.0, "folded water");
        assert_eq!(foam_coverage(f32::NAN, full), 0.0);
        // Off means off, however folded the surface.
        for j in [-50.0f32, -1.5, 0.0, 1.0] {
            assert_eq!(foam_coverage(j, foam_threshold(0.0)), 0.0, "J {j} at setting 0");
        }
        // More foam at a higher setting, never less.
        for j in [-0.5f32, 0.2, 0.7, 0.95] {
            let mut last = 0.0;
            for s in [0.0f32, 0.3, 0.6, 1.0] {
                let c = foam_coverage(j, foam_threshold(s));
                assert!((0.0..=1.0).contains(&c) && c >= last, "J {j}, setting {s}: {c} after {last}");
                last = c;
            }
        }
    }

    #[test]
    fn shore_fade_is_a_smoothstep_from_the_shoreline() {
        let d = 4.0;
        assert_eq!(shore_fade(0.0, d), 0.0, "the shoreline");
        assert_eq!(shore_fade(-3.0, d), 0.0, "dry land");
        assert_eq!(shore_fade(d, d), 1.0);
        assert_eq!(shore_fade(25.0, d), 1.0);
        assert_eq!(shore_fade(f32::INFINITY, d), 1.0);
        assert_eq!(shore_fade(f32::NAN, d), 1.0, "unknown ground is deep water");
        assert!((shore_fade(0.5 * d, d) - 0.5).abs() < 1e-6);
        assert!((shore_fade(0.25 * d, d) - 0.15625).abs() < 1e-6, "3/16 - 2/64");
        // Rises steadily, with zero slope at both ends.
        let mut last = 0.0;
        for i in 1..=100 {
            let f = shore_fade(d * i as f32 / 100.0, d);
            assert!(f > last, "step {i}");
            last = f;
        }
        assert!(shore_fade(1e-3 * d, d) < 1e-5 && shore_fade(0.999 * d, d) > 1.0 - 1e-5);
        // No fade depth: a step at the shoreline.
        assert_eq!(shore_fade(0.01, 0.0), 1.0);
        assert_eq!(shore_fade(-0.01, 0.0), 0.0);
        assert_eq!(shore_fade(0.01, f32::NAN), 1.0);
    }

    #[test]
    fn normals_from_slopes_are_unit_and_lean_downhill() {
        assert_eq!(normal_from_slopes(0.0, 0.0), [0.0, 1.0, 0.0]);
        for (sx, sz) in [(0.3f32, 0.0f32), (-0.2, 0.45), (1.0, -1.0), (5.0, 2.0)] {
            let n = normal_from_slopes(sx, sz);
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((length - 1.0).abs() < 1e-6, "({sx}, {sz}): length {length}");
            assert!(n[1] > 0.0);
            // Perpendicular to both tangents (1, sx, 0) and (0, sz, 1).
            assert!((n[0] + sx * n[1]).abs() < 1e-6 && (n[2] + sz * n[1]).abs() < 1e-6, "({sx}, {sz}): {n:?}");
        }
    }
}

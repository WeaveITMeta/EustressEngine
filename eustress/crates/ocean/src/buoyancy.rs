//! Buoyancy of a rigid body in the sea: pure maths on points, volumes and
//! water heights, with no engine types, so the physics runtime and these
//! tests share it.
//!
//! # Samples
//!
//! A body's volume is shared out among sample cells. [`box_samples`] cuts a
//! box (a cuboid collider, or the local bounding box of any other shape)
//! into an n x n x n lattice of equal cells and gives each cell's centre in
//! the world, its volume and its vertical extent. A cell of sizes (a, b, c)
//! along the box's axes, rotated so that those axes point along u, v and w,
//! spans |u_y| a + |v_y| b + |w_y| c vertically: the height of its shadow on
//! a vertical line. [`scale_volumes`] rescales the cells to add up to the
//! shape's true volume (a sphere fills about half of its box).
//!
//! # Lift
//!
//! Archimedes: displaced water pushes up with its own weight. A sample at
//! height y with vertical extent h, under a water surface at s, is
//! submerged by the fraction
//!
//! f = clamp((s - (y - h/2)) / h, 0, 1),
//!
//! exactly the part of an upright cell under water, and is lifted by
//! rho g V f along +Y at its centre (Eustress's gravity points down -Y).
//! Summed, the lifts are rho g times the volume under water; applied at the
//! centres, they give the torque that rights a stable hull and capsizes an
//! unstable one. An upright box floats at exactly its density ratio; a
//! tilted one approximately, since its cells' vertical extents overlap.
//!
//! # Drag
//!
//! Water resists motion through it. Each submerged sample takes
//!
//! -rho V f c (v - (0, w, 0)),
//!
//! with v the body's velocity at the sample, w the rate of rise of the
//! surface there and c the drag rate (1/s). The model is linear in the
//! velocity, which keeps it stable under an explicit step: a body's velocity
//! relative to the water decays at the rate c rho V_under / m, and one step
//! of dt neither overshoots nor diverges while that rate times dt is below
//! 1. A floating body displaces its own mass (rho V_under = m), so the rate
//! is c itself: with the default c = 1/s and a 60 Hz step, c dt = 1/60.
//! Held fully under, a body of relative density r decays at c / r, still
//! below 1/dt at the default for any r above 1/60; [`stable_drag`] caps the
//! rate for lighter bodies. The water's horizontal orbital motion is left
//! out: floating bodies are damped toward rest horizontally rather than
//! carried by the waves.

/// Density of seawater (kg/m^3).
pub const SEAWATER_DENSITY: f32 = 1025.0;
/// Default drag rate c (1/s): c dt = 1/60 at 60 Hz.
pub const DEFAULT_DRAG: f32 = 1.0;

/// One cell of a body: where its centre is, how much volume it holds and
/// how far it reaches vertically.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuoyancySample {
    /// Centre of the cell in the world (m).
    pub point: [f32; 3],
    /// Volume of the cell (m^3).
    pub volume: f32,
    /// Vertical extent of the cell (m).
    pub height: f32,
}

/// The water above or below a point: the world height of its surface and
/// the surface's rate of rise there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterColumn {
    /// World height of the water surface (m).
    pub surface: f32,
    /// Rate of rise of the surface (m/s).
    pub vertical_velocity: f32,
}

impl WaterColumn {
    /// Still water with its surface at world height `surface` (m).
    pub fn still(surface: f32) -> Self {
        Self { surface, vertical_velocity: 0.0 }
    }
}

/// A force (N) to apply at a world point (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuoyancyForce {
    /// The force (N).
    pub force: [f32; 3],
    /// Where it acts in the world (m).
    pub point: [f32; 3],
}

/// The images of the unit X, Y and Z axes under the rotation of quaternion
/// (x, y, z, w), normalised; the identity when it has no length.
fn rotation_axes(rotation: [f32; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = rotation.map(|v| v as f64);
    let length_sq = x * x + y * y + z * z + w * w;
    let (x, y, z, w) = if length_sq.is_finite() && length_sq > 1e-12 {
        let inverse = 1.0 / length_sq.sqrt();
        (x * inverse, y * inverse, z * inverse, w * inverse)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)],
        [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)],
        [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// The n x n x n lattice of equal cells of a box with half extents
/// `half_extents` (m, along its own axes), turned by the quaternion
/// `rotation` (x, y, z, w) and centred at world `center`: each cell's centre,
/// its volume (the box's volume over n^3) and its vertical extent (the sum
/// over the box's axes of |axis_y| times the cell's size along that axis).
/// n = 0 counts as 1; half extents are taken as magnitudes, and a
/// non-finite one as 0.
pub fn box_samples(center: [f32; 3], rotation: [f32; 4], half_extents: [f32; 3], n: usize) -> Vec<BuoyancySample> {
    let n = n.max(1);
    let axes = rotation_axes(rotation);
    let half = half_extents.map(|h| if h.is_finite() { h.abs() as f64 } else { 0.0 });
    let cell = half.map(|h| 2.0 * h / n as f64);
    let volume = (cell[0] * cell[1] * cell[2]) as f32;
    let height = (axes[0][1].abs() * cell[0] + axes[1][1].abs() * cell[1] + axes[2][1].abs() * cell[2]) as f32;
    let c = center.map(|v| v as f64);
    let mut samples = Vec::with_capacity(n * n * n);
    for i in 0..n {
        let u = -half[0] + cell[0] * (i as f64 + 0.5);
        for j in 0..n {
            let v = -half[1] + cell[1] * (j as f64 + 0.5);
            for k in 0..n {
                let w = -half[2] + cell[2] * (k as f64 + 0.5);
                let point = [0, 1, 2].map(|d| (c[d] + axes[0][d] * u + axes[1][d] * v + axes[2][d] * w) as f32);
                samples.push(BuoyancySample { point, volume, height });
            }
        }
    }
    samples
}

/// Rescales the samples' volumes, in proportion, to add up to
/// `total_volume` (m^3): for a collider whose true volume differs from its
/// box's. Samples of no volume at all share the total equally. A total that
/// is negative or not finite leaves the samples as they are.
pub fn scale_volumes(samples: &mut [BuoyancySample], total_volume: f32) {
    if samples.is_empty() || !(total_volume >= 0.0) || !total_volume.is_finite() {
        return;
    }
    let sum: f64 = samples.iter().map(|s| s.volume as f64).sum();
    if sum > 0.0 && sum.is_finite() {
        let scale = total_volume as f64 / sum;
        for s in samples.iter_mut() {
            s.volume = (s.volume as f64 * scale) as f32;
        }
    } else {
        let each = total_volume / samples.len() as f32;
        for s in samples.iter_mut() {
            s.volume = each;
        }
    }
}

/// The fraction of a sample under water, in [0, 1]:
/// clamp((surface - (y - height/2)) / height, 0, 1). A sample of no height
/// is wholly in (its centre below the surface) or out; a NaN surface leaves
/// it out.
pub fn submerged_fraction(sample: &BuoyancySample, water: WaterColumn) -> f32 {
    let depth = water.surface - (sample.point[1] - 0.5 * sample.height);
    let fraction = if sample.height > 0.0 {
        depth / sample.height
    } else if depth > 0.0 {
        1.0
    } else {
        0.0
    };
    if fraction > 0.0 {
        fraction.min(1.0)
    } else {
        0.0
    }
}

/// The volume under water (m^3) of samples standing in the given water
/// columns, one per sample: the sum of volume times submerged fraction.
pub fn submerged_volume(samples: &[BuoyancySample], water: &[WaterColumn]) -> f32 {
    samples.iter().zip(water).map(|(s, w)| (s.volume * submerged_fraction(s, *w)) as f64).sum::<f64>() as f32
}

/// The force on one sample standing in `water` while the body moves at
/// `velocity` (m/s) there, under `gravity` (m/s^2, its magnitude) in water
/// of `density` (kg/m^3) with drag rate `drag` (1/s): lift
/// rho g V f along +Y plus drag -rho V f c (v - (0, w, 0)), at the sample's
/// centre. None when the sample is out of the water.
pub fn sample_force(
    sample: &BuoyancySample,
    water: WaterColumn,
    velocity: [f32; 3],
    gravity: f32,
    density: f32,
    drag: f32,
) -> Option<BuoyancyForce> {
    let fraction = submerged_fraction(sample, water);
    (fraction > 0.0).then(|| force_on(sample, water, fraction, velocity, gravity, density, drag))
}

fn force_on(
    sample: &BuoyancySample,
    water: WaterColumn,
    fraction: f32,
    velocity: [f32; 3],
    gravity: f32,
    density: f32,
    drag: f32,
) -> BuoyancyForce {
    // Mass of the displaced water (kg) and the drag it exerts per unit of
    // relative velocity (kg/s).
    let displaced = density * sample.volume * fraction;
    let resistance = displaced * drag;
    let relative = [velocity[0], velocity[1] - water.vertical_velocity, velocity[2]];
    BuoyancyForce {
        force: [-resistance * relative[0], displaced * gravity - resistance * relative[1], -resistance * relative[2]],
        point: sample.point,
    }
}

/// The forces of the water on a body's samples: for each sample in the water
/// that `water` (x, z) describes there, the lift and drag of
/// [`sample_force`], with `point_velocity` giving the body's velocity at a
/// world point (asked only for samples in the water). Samples out of the
/// water get no force and no entry.
pub fn buoyancy_forces(
    samples: &[BuoyancySample],
    water: impl Fn(f32, f32) -> WaterColumn,
    point_velocity: impl Fn([f32; 3]) -> [f32; 3],
    gravity: f32,
    density: f32,
    drag: f32,
) -> Vec<BuoyancyForce> {
    samples
        .iter()
        .filter_map(|sample| {
            let column = water(sample.point[0], sample.point[2]);
            let fraction = submerged_fraction(sample, column);
            (fraction > 0.0).then(|| force_on(sample, column, fraction, point_velocity(sample.point), gravity, density, drag))
        })
        .collect()
}

/// The drag rate (1/s) to use for a body of `mass` (kg) with
/// `submerged_volume` (m^3) in water of `density` (kg/m^3), stepped by `dt`
/// (s): `drag`, lowered where needed so that the rate at which the drag
/// slows the body, drag * density * submerged_volume / mass, times dt is at
/// most 1, so one explicit step never reverses its motion relative to the
/// water. Only bodies far lighter than water held under need it. A drag
/// that is not positive gives 0; a mass, volume or step that is not
/// positive leaves `drag` as it is.
pub fn stable_drag(drag: f32, mass: f32, density: f32, submerged_volume: f32, dt: f32) -> f32 {
    if !(drag > 0.0) {
        return 0.0;
    }
    let resistance_step = density * submerged_volume * dt;
    if !(resistance_step > 0.0) || !(mass > 0.0) {
        return drag;
    }
    drag.min(mass / resistance_step)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = 9.80665;
    const RHO: f32 = SEAWATER_DENSITY;
    const LEVEL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    /// Half extents of a 2 m x 1 m x 3 m box: wide and flat enough to float
    /// upright, stable in roll and in pitch.
    const HULL: [f32; 3] = [1.0, 0.5, 1.5];

    /// Quaternion (x, y, z, w) of a turn of `degrees` about a unit axis.
    fn about(axis: [f32; 3], degrees: f32) -> [f32; 4] {
        let (s, c) = (0.5 * degrees.to_radians()).sin_cos();
        [axis[0] * s, axis[1] * s, axis[2] * s, c]
    }

    fn box_volume(half: [f32; 3]) -> f64 {
        8.0 * half[0] as f64 * half[1] as f64 * half[2] as f64
    }

    fn total(forces: &[BuoyancyForce]) -> [f64; 3] {
        let mut sum = [0.0f64; 3];
        for f in forces {
            for (s, c) in sum.iter_mut().zip(f.force) {
                *s += c as f64;
            }
        }
        sum
    }

    /// Sum of (p - centre) x F.
    fn torque_about(forces: &[BuoyancyForce], centre: [f32; 3]) -> [f64; 3] {
        let mut sum = [0.0f64; 3];
        for f in forces {
            let r = [0, 1, 2].map(|d| f.point[d] as f64 - centre[d] as f64);
            let v = f.force.map(|c| c as f64);
            sum[0] += r[1] * v[2] - r[2] * v[1];
            sum[1] += r[2] * v[0] - r[0] * v[2];
            sum[2] += r[0] * v[1] - r[1] * v[0];
        }
        sum
    }

    /// Lift (N) on a box centred at (0, y, 0) in still water at height 0,
    /// at rest, with no drag.
    fn lift(half: [f32; 3], rotation: [f32; 4], y: f32) -> f64 {
        let samples = box_samples([0.0, y, 0.0], rotation, half, 3);
        total(&buoyancy_forces(&samples, |_, _| WaterColumn::still(0.0), |_| [0.0; 3], G, RHO, 0.0))[1]
    }

    /// The centre height at which a box of relative density `ratio` floats
    /// in still water at 0: bisection on lift minus weight, which falls as
    /// the box rises, between fully under and fully out.
    fn floating_height(half: [f32; 3], rotation: [f32; 4], ratio: f64) -> f32 {
        let weight = ratio * RHO as f64 * box_volume(half) * G as f64;
        let reach = half[0] + half[1] + half[2] + 1.0;
        let (mut lo, mut hi) = (-reach, reach);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if lift(half, rotation, mid) > weight {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    #[test]
    fn box_samples_fill_the_box() {
        let half = [1.0f32, 0.5, 2.0];
        let center = [3.0f32, -2.0, 7.5];
        let tilted = about([0.48, 0.6, 0.64], 37.0);
        for rotation in [LEVEL, tilted] {
            let samples = box_samples(center, rotation, half, 3);
            assert_eq!(samples.len(), 27);
            let volume: f64 = samples.iter().map(|s| s.volume as f64).sum();
            assert!((volume / box_volume(half) - 1.0).abs() < 1e-6, "volume {volume}");
            for d in 0..3 {
                let mean = samples.iter().map(|s| s.point[d] as f64).sum::<f64>() / 27.0;
                assert!((mean - center[d] as f64).abs() < 1e-5, "centroid axis {d}: {mean}");
            }
        }
        // Upright: each cell stands a third of the box's height.
        let upright = box_samples(center, LEVEL, half, 3);
        assert!(upright.iter().all(|s| (s.height - 1.0 / 3.0).abs() < 1e-6));
        // A quarter turn about X stands the box's Z cells upright: 4/3 m.
        let on_end = box_samples(center, about([1.0, 0.0, 0.0], 90.0), half, 3);
        assert!(on_end.iter().all(|s| (s.height - 4.0 / 3.0).abs() < 1e-5), "{}", on_end[0].height);
        // Turned about Y the vertical is untouched.
        let yawed = box_samples(center, about([0.0, 1.0, 0.0], 61.0), half, 3);
        assert!(yawed.iter().all(|s| (s.height - 1.0 / 3.0).abs() < 1e-6));
        // The cells reach exactly the box's lowest point and no lower.
        let lowest = tilted_lowest_point(center, tilted, half);
        let reach = box_samples(center, tilted, half, 3)
            .iter()
            .map(|s| s.point[1] - 0.5 * s.height)
            .fold(f32::MAX, f32::min);
        assert!((reach - lowest).abs() < 1e-5, "cells reach {reach}, box {lowest}");
        // n = 0 is one cell holding the whole box.
        let one = box_samples(center, tilted, half, 0);
        assert_eq!(one.len(), 1);
        assert!((one[0].volume as f64 / box_volume(half) - 1.0).abs() < 1e-6);
        assert!(one[0].point.iter().zip(center).all(|(p, c)| (p - c).abs() < 1e-6));
    }

    /// The lowest corner of a turned box.
    fn tilted_lowest_point(center: [f32; 3], rotation: [f32; 4], half: [f32; 3]) -> f32 {
        let axes = rotation_axes(rotation);
        let drop: f64 = (0..3).map(|a| axes[a][1].abs() * half[a] as f64).sum();
        (center[1] as f64 - drop) as f32
    }

    #[test]
    fn scaled_volumes_add_up_to_the_true_volume() {
        let mut samples = box_samples([0.0; 3], about([0.0, 0.0, 1.0], 15.0), [1.0, 1.0, 1.0], 3);
        let sphere = 4.0 / 3.0 * std::f32::consts::PI;
        scale_volumes(&mut samples, sphere);
        let sum: f64 = samples.iter().map(|s| s.volume as f64).sum();
        assert!((sum / sphere as f64 - 1.0).abs() < 1e-6, "{sum}");
        assert!(samples.iter().all(|s| (s.volume - samples[0].volume).abs() < 1e-7), "in proportion");
        // A box of no volume shares the total out equally.
        let mut flat = box_samples([0.0; 3], LEVEL, [1.0, 0.0, 1.0], 2);
        scale_volumes(&mut flat, 2.0);
        assert!(flat.iter().all(|s| s.volume == 0.25));
        // A total that makes no sense changes nothing.
        let before = flat.clone();
        scale_volumes(&mut flat, f32::NAN);
        scale_volumes(&mut flat, -1.0);
        assert_eq!(flat, before);
    }

    /// Fully under water a box is lifted by rho g V within 0.1%, straight up
    /// and through its centre, however it is turned.
    #[test]
    fn full_submersion_lifts_the_weight_of_the_displaced_water() {
        let center = [1.0f32, 2.0, 3.0];
        for rotation in [LEVEL, about([0.6, 0.0, 0.8], 33.0), about([0.0, 1.0, 0.0], 80.0)] {
            let samples = box_samples(center, rotation, HULL, 3);
            let forces = buoyancy_forces(&samples, |_, _| WaterColumn::still(100.0), |_| [0.0; 3], G, RHO, DEFAULT_DRAG);
            assert_eq!(forces.len(), 27);
            let f = total(&forces);
            let expect = RHO as f64 * G as f64 * box_volume(HULL);
            assert!((f[1] / expect - 1.0).abs() < 1e-3, "lift {} vs {expect}", f[1]);
            assert!(f[0].abs() < 1e-9 && f[2].abs() < 1e-9, "a body at rest feels no drag: {f:?}");
            let torque = torque_about(&forces, center);
            assert!(torque.iter().all(|t| t.abs() < 1e-4 * expect), "lift through the centre: {torque:?}");
        }
    }

    /// An upright box of relative density r floats with the fraction r of
    /// its height under water, within 3% (the cells make it exact up to
    /// rounding).
    #[test]
    fn upright_box_floats_at_its_density_ratio() {
        for ratio in [0.3f64, 0.5, 0.8] {
            let y = floating_height(HULL, LEVEL, ratio);
            let fraction = (HULL[1] as f64 - y as f64) / (2.0 * HULL[1] as f64);
            assert!((fraction / ratio - 1.0).abs() < 0.03, "relative density {ratio}: floats {fraction} under");
        }
    }

    /// A box denser than water is lifted by less than it weighs even fully
    /// under, so it sinks.
    #[test]
    fn denser_box_sinks() {
        for ratio in [1.2f64, 7.85] {
            let weight = ratio * RHO as f64 * box_volume(HULL) * G as f64;
            let most = lift(HULL, about([0.3, 0.9, 0.3], 25.0), -20.0);
            assert!(most < weight, "relative density {ratio}: lift {most} vs weight {weight}");
            assert!(most > 0.0);
        }
    }

    /// Heeled 20 degrees either way about either horizontal axis, at the
    /// depth where it floats so turned, a box that floats upright stably is
    /// turned back toward level: the torque about its centre along the
    /// heel's axis has the opposite sign to the heel, with a righting arm
    /// (torque over weight) of at least 5 cm, and none about the other axes.
    #[test]
    fn heeled_box_is_righted() {
        for ratio in [0.35f64, 0.5, 0.7] {
            let weight = ratio * RHO as f64 * box_volume(HULL) * G as f64;
            for (axis, index) in [([0.0f32, 0.0, 1.0], 2usize), ([1.0, 0.0, 0.0], 0)] {
                for heel in [20.0f32, -20.0] {
                    let rotation = about(axis, heel);
                    let y = floating_height(HULL, rotation, ratio);
                    let samples = box_samples([0.0, y, 0.0], rotation, HULL, 3);
                    let forces = buoyancy_forces(&samples, |_, _| WaterColumn::still(0.0), |_| [0.0; 3], G, RHO, 0.0);
                    let torque = torque_about(&forces, [0.0, y, 0.0]);
                    let arm = torque[index] / weight;
                    assert!(arm * (heel as f64) < 0.0, "density {ratio}, heel {heel} about {axis:?}: torque {torque:?}");
                    assert!(arm.abs() > 0.05, "density {ratio}, heel {heel} about {axis:?}: righting arm {arm} m");
                    for (other, t) in torque.iter().enumerate() {
                        if other != index {
                            assert!(t.abs() < 1e-3 * torque[index].abs(), "density {ratio}, heel {heel}: stray torque {torque:?}");
                        }
                    }
                }
            }
        }
    }

    /// Drag is -rho V f c times the velocity relative to the water: it
    /// opposes motion through the water, pulls a body at rest along with
    /// rising water, and scales with how much of the sample is under.
    #[test]
    fn drag_opposes_motion_through_the_water() {
        let sample = BuoyancySample { point: [0.0, -5.0, 0.0], volume: 0.5, height: 1.0 };
        let rising = WaterColumn { surface: 0.0, vertical_velocity: 0.75 };
        let velocity = [1.0f32, -2.0, 0.5];
        // No gravity: drag alone.
        let f = sample_force(&sample, rising, velocity, 0.0, RHO, 2.0).expect("under water").force;
        let relative = [1.0f32, -2.75, 0.5];
        let k = RHO * 0.5 * 2.0;
        for d in 0..3 {
            assert!((f[d] - -k * relative[d]).abs() < 1e-3, "axis {d}: {f:?}");
        }
        let along: f32 = (0..3).map(|d| f[d] * relative[d]).sum();
        assert!(along < 0.0);
        // At rest in rising water: dragged up.
        let lifted = sample_force(&sample, rising, [0.0; 3], 0.0, RHO, 2.0).unwrap().force;
        assert!(lifted[1] > 0.0 && lifted[0] == 0.0 && lifted[2] == 0.0);
        // Half under: half the drag.
        let half_under = BuoyancySample { point: [0.0, 0.0, 0.0], ..sample };
        let h = sample_force(&half_under, WaterColumn::still(0.0), velocity, 0.0, RHO, 2.0).unwrap().force;
        let whole = sample_force(&sample, WaterColumn::still(0.0), velocity, 0.0, RHO, 2.0).unwrap().force;
        for d in 0..3 {
            assert!((h[d] - 0.5 * whole[d]).abs() < 1e-3, "axis {d}: {h:?} vs {whole:?}");
        }
        // The closure form gives the same.
        let via = buoyancy_forces(&[sample], |_, _| rising, |_| velocity, 0.0, RHO, 2.0);
        assert_eq!(via, vec![BuoyancyForce { force: f, point: sample.point }]);
    }

    /// On a wave, a box centred at the rest level is lifted more on a crest
    /// than in a trough: a 1 m high box under a 1 m high, 20 m long wave is
    /// nearly all under on the crest and nearly all out in the trough.
    #[test]
    fn a_crest_lifts_more_than_a_trough() {
        let k = std::f32::consts::TAU / 20.0;
        let wave = |x: f32, _z: f32| WaterColumn { surface: 0.5 * (k * x).cos(), vertical_velocity: 0.0 };
        let half = [1.0f32, 0.5, 1.0];
        let full = RHO as f64 * G as f64 * box_volume(half);
        let lift_at = |x: f32| {
            let samples = box_samples([x, 0.0, 0.0], LEVEL, half, 3);
            total(&buoyancy_forces(&samples, wave, |_| [0.0; 3], G, RHO, 0.0))[1] / full
        };
        let (crest, trough) = (lift_at(0.0), lift_at(10.0));
        assert!(crest > 0.9 && trough < 0.1, "crest {crest}, trough {trough} of full lift");
        assert!(crest > trough);
    }

    /// Nothing out of the water feels it: no forces at all above the
    /// surface, and none for a box just clear of it.
    #[test]
    fn nothing_above_the_water_is_pushed() {
        let half = [1.0f32, 0.5, 1.0];
        let clear = box_samples([4.0, 0.5 + 1e-3, -2.0], LEVEL, half, 3);
        assert!(buoyancy_forces(&clear, |_, _| WaterColumn::still(0.0), |_| [1.0, -3.0, 0.0], G, RHO, DEFAULT_DRAG).is_empty());
        let high = box_samples([0.0, 40.0, 0.0], about([0.0, 0.6, 0.8], 70.0), half, 3);
        let waves = |x: f32, z: f32| WaterColumn { surface: 2.0 * (0.1 * x).sin() * (0.07 * z).cos(), vertical_velocity: 3.0 };
        assert!(buoyancy_forces(&high, waves, |_| [0.0; 3], G, RHO, DEFAULT_DRAG).is_empty());
        let nan = WaterColumn { surface: f32::NAN, vertical_velocity: 0.0 };
        assert!(buoyancy_forces(&clear, |_, _| nan, |_| [0.0; 3], G, RHO, DEFAULT_DRAG).is_empty());
        // Just dipped in: a force, and only on the bottom layer.
        let dipped = box_samples([4.0, 0.5 - 1e-2, -2.0], LEVEL, half, 3);
        assert_eq!(buoyancy_forces(&dipped, |_, _| WaterColumn::still(0.0), |_| [0.0; 3], G, RHO, 0.0).len(), 9);
    }

    #[test]
    fn submerged_volume_sums_the_parts_under_water() {
        let samples = box_samples([0.0, 0.0, 0.0], LEVEL, HULL, 3);
        let still = vec![WaterColumn::still(0.0); samples.len()];
        let under = submerged_volume(&samples, &still) as f64;
        assert!((under / (0.5 * box_volume(HULL)) - 1.0).abs() < 1e-6, "{under}");
        let deep = vec![WaterColumn::still(10.0); samples.len()];
        assert!((submerged_volume(&samples, &deep) as f64 / box_volume(HULL) - 1.0).abs() < 1e-6);
    }

    /// The drag rate is kept where one step cannot reverse the motion, and
    /// lowered to exactly that limit for a body far lighter than water held
    /// under.
    #[test]
    fn stable_drag_caps_only_what_would_overshoot() {
        let dt = 1.0 / 60.0;
        // A floating body displaces its own mass: the default is kept.
        assert_eq!(stable_drag(DEFAULT_DRAG, 500.0, RHO, 500.0 / RHO, dt), DEFAULT_DRAG);
        // A beach ball, 0.5% of water's density, held fully under.
        let (volume, mass) = (0.1f32, 0.005 * RHO * 0.1);
        let capped = stable_drag(DEFAULT_DRAG, mass, RHO, volume, dt);
        assert!(capped < DEFAULT_DRAG);
        assert!((capped * RHO * volume * dt / mass - 1.0).abs() < 1e-5, "rate times dt {}", capped * RHO * volume * dt / mass);
        // Nothing under, or no mass known: unchanged; no drag: none.
        assert_eq!(stable_drag(DEFAULT_DRAG, mass, RHO, 0.0, dt), DEFAULT_DRAG);
        assert_eq!(stable_drag(DEFAULT_DRAG, f32::NAN, RHO, volume, dt), DEFAULT_DRAG);
        assert_eq!(stable_drag(-1.0, mass, RHO, volume, dt), 0.0);
        assert_eq!(stable_drag(f32::NAN, mass, RHO, volume, dt), 0.0);
    }
}

//! Velocity-predictive streaming — prefetch where the camera is GOING, not
//! only where it is.
//!
//! ## Why proximity alone is not enough
//!
//! [`super::residency`] loads a symmetric box around the camera. That is
//! correct but always late: at speed the camera crosses a 256 m cell boundary
//! and only THEN is the next cell queued, scanned out of Fjall and spawned.
//! The camera outruns the loader and flies into empty space.
//!
//! Prediction fixes the ORDERING problem, not the throughput one. The same
//! cells get loaded; they get loaded in the order the camera will actually
//! need them, and the region is stretched along the heading so its far edge
//! arrives before the camera does.
//!
//! ## Shape of the solution
//!
//! * [`CameraMotion`] folds per-tick position into an EMA-smoothed velocity.
//!   Smoothing matters: an editor camera jitters, and a raw heading would
//!   re-plan every frame and thrash the queue.
//! * [`predicted_focus`] projects forward by `lead_time`, clamped so a fast
//!   dolly cannot request a corridor across the world.
//! * [`cell_priority`] scores candidates in SECONDS-to-reach, so the queue
//!   drains in arrival order rather than insertion order.
//! * [`course_changed`] detects a genuine turn, which is the cue to re-plan
//!   and CANCEL prefetch no longer on the path. Without cancellation a U-turn
//!   leaves the queue grinding through cells behind the camera while the ones
//!   ahead wait their turn.
//!
//! All of it is pure math over `Vec3` plus one resource holding two vectors,
//! so it unit-tests without a `World`, a camera, or a database.

use bevy::prelude::*;

/// Speed below which the camera counts as stationary and prediction switches
/// off entirely, falling back to pure proximity. Metres/second, well under a
/// slow walk, so drift and numerical noise can never fabricate a heading.
pub const STATIONARY_SPEED: f32 = 0.25;

/// Weight on a cell's LATERAL offset from the travel line. At 1.0 a second
/// sideways would rank equal to a second ahead; off-path cells deserve worse
/// than that because the camera may never reach them at all.
const LATERAL_WEIGHT: f32 = 2.0;

/// Multiplier on the time score for cells BEHIND the camera. They stay
/// eligible — a reversal must not hit a cliff — but rank after everything
/// ahead.
const BEHIND_PENALTY: f32 = 4.0;

/// Camera motion sampled over time. The input to every prediction below.
#[derive(Resource, Debug, Clone)]
pub struct CameraMotion {
    /// Position at the previous sample; `None` until the first one lands.
    last_pos: Option<Vec3>,
    /// EMA-smoothed velocity, metres/second.
    pub velocity: Vec3,
    /// Unit travel direction, or `Vec3::ZERO` when stationary.
    pub heading: Vec3,
    /// Smoothed speed, metres/second.
    pub speed: f32,
    /// Heading captured when the current prefetch plan was built.
    /// [`CameraMotion::course_changed`] measures against this so a re-plan
    /// follows a real turn instead of every frame of micro-jitter.
    pub planned_heading: Vec3,
}

impl Default for CameraMotion {
    fn default() -> Self {
        Self {
            last_pos: None,
            velocity: Vec3::ZERO,
            heading: Vec3::ZERO,
            speed: 0.0,
            planned_heading: Vec3::ZERO,
        }
    }
}

impl CameraMotion {
    /// Fold one position sample into the smoothed velocity.
    ///
    /// `smoothing` is the EMA weight given to the new sample, `0.0..=1.0`:
    /// higher reacts faster and is noisier. `dt` is guarded so a paused or
    /// rewound clock cannot produce infinities.
    ///
    /// A teleport — Space switch, camera "go to", a scripted jump — would
    /// otherwise read as enormous velocity and prefetch a corridor across the
    /// world, so a jump past `teleport_threshold` discards the history rather
    /// than folding it in.
    pub fn sample(&mut self, pos: Vec3, dt: f32, smoothing: f32, teleport_threshold: f32) {
        let Some(prev) = self.last_pos else {
            self.last_pos = Some(pos);
            return;
        };
        self.last_pos = Some(pos);
        let delta = pos - prev;
        if delta.length() >= teleport_threshold {
            // A discontinuity, not motion. Drop the estimate entirely.
            self.velocity = Vec3::ZERO;
            self.heading = Vec3::ZERO;
            self.speed = 0.0;
            self.planned_heading = Vec3::ZERO;
            return;
        }
        if dt <= f32::EPSILON {
            return;
        }
        let instant = delta / dt;
        let a = smoothing.clamp(0.0, 1.0);
        self.velocity = self.velocity * (1.0 - a) + instant * a;
        self.speed = self.velocity.length();
        self.heading = if self.speed > STATIONARY_SPEED {
            self.velocity / self.speed
        } else {
            Vec3::ZERO
        };
    }

    /// Moving fast enough for prediction to mean anything.
    pub fn is_moving(&self) -> bool {
        self.speed > STATIONARY_SPEED && self.heading != Vec3::ZERO
    }

    /// Record the heading this plan was built for, so the next
    /// [`CameraMotion::course_changed`] compares against it.
    pub fn mark_planned(&mut self) {
        self.planned_heading = self.heading;
    }

    /// Has the heading diverged from the planned one past `cos_threshold`
    /// (a dot product, so 0.85 is roughly 32 degrees)?
    ///
    /// False while stationary: stopping is not a course change, and treating
    /// it as one would re-plan every time the user releases a key.
    pub fn course_changed(&self, cos_threshold: f32) -> bool {
        if !self.is_moving() || self.planned_heading == Vec3::ZERO {
            return false;
        }
        self.heading.dot(self.planned_heading) < cos_threshold
    }
}

/// Where the camera is predicted to be `lead_time` seconds from now.
///
/// Clamped to `max_lead` so prefetch cost stays proportional to the
/// configured radius rather than to whatever speed the user managed to reach.
pub fn predicted_focus(pos: Vec3, motion: &CameraMotion, lead_time: f32, max_lead: f32) -> Vec3 {
    if !motion.is_moving() {
        return pos;
    }
    let lead = motion.velocity * lead_time.max(0.0);
    if lead.length() > max_lead {
        pos + motion.heading * max_lead
    } else {
        pos + lead
    }
}

/// Score a candidate cell: LOWER loads sooner. The unit is seconds-to-reach,
/// so the number means something rather than being an arbitrary rank.
///
/// Stationary falls back to plain distance, reproducing the old proximity
/// ordering exactly.
pub fn cell_priority(cell_center: Vec3, cam: Vec3, motion: &CameraMotion) -> f32 {
    let d = cell_center - cam;
    if !motion.is_moving() {
        return d.length();
    }
    let speed = motion.speed.max(STATIONARY_SPEED);
    // Split the offset into "along the heading" and "off to the side".
    let along = d.dot(motion.heading);
    let lateral = (d - motion.heading * along).length();
    let along_secs = if along >= 0.0 {
        along / speed
    } else {
        // Behind: reachable if the user reverses, but not soon.
        (-along / speed) * BEHIND_PENALTY
    };
    along_secs + (lateral / speed) * LATERAL_WEIGHT
}

/// Should this cell be in the desired set at all?
///
/// Keeps the near ball — everything within `load_radius`, heading or not,
/// because the camera can always turn — and adds a corridor of
/// `prefetch_radius` around the predicted position. The union is what makes
/// the resident region stretch forward without abandoning the surroundings.
pub fn cell_wanted(
    cell_center: Vec3,
    cam: Vec3,
    predicted: Vec3,
    load_radius: f32,
    prefetch_radius: f32,
) -> bool {
    if (cell_center - cam).length_squared() <= load_radius * load_radius {
        return true;
    }
    (cell_center - predicted).length_squared() <= prefetch_radius * prefetch_radius
}

/// Eviction distance for an entity, given where it sits relative to travel.
///
/// Behind the camera evicts sooner than ahead: that geometry is being left,
/// and holding it spends budget the forward direction needs. A reversal
/// re-loads it, which is exactly why `behind` must stay generous enough to
/// survive an ordinary turn rather than being tuned to the theoretical floor.
pub fn directional_evict_radius(
    entity_pos: Vec3,
    cam: Vec3,
    motion: &CameraMotion,
    ahead: f32,
    behind: f32,
) -> f32 {
    if !motion.is_moving() {
        return ahead;
    }
    if (entity_pos - cam).dot(motion.heading) < 0.0 {
        behind
    } else {
        ahead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moving(dir: Vec3, speed: f32) -> CameraMotion {
        let mut m = CameraMotion::default();
        m.velocity = dir.normalize() * speed;
        m.speed = speed;
        m.heading = dir.normalize();
        m
    }

    #[test]
    fn stationary_camera_predicts_no_lead() {
        let m = CameraMotion::default();
        let p = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(predicted_focus(p, &m, 2.0, 1000.0), p);
    }

    #[test]
    fn lead_is_clamped_to_max() {
        let m = moving(Vec3::X, 500.0);
        let got = predicted_focus(Vec3::ZERO, &m, 10.0, 100.0);
        assert!((got.x - 100.0).abs() < 0.001, "expected clamp to 100, got {got:?}");
    }

    #[test]
    fn cells_ahead_outrank_cells_behind() {
        let m = moving(Vec3::X, 50.0);
        let ahead = cell_priority(Vec3::new(500.0, 0.0, 0.0), Vec3::ZERO, &m);
        let behind = cell_priority(Vec3::new(-500.0, 0.0, 0.0), Vec3::ZERO, &m);
        assert!(ahead < behind, "ahead {ahead} must sort before behind {behind}");
    }

    #[test]
    fn on_path_outranks_lateral_at_equal_distance() {
        let m = moving(Vec3::X, 50.0);
        let on_path = cell_priority(Vec3::new(500.0, 0.0, 0.0), Vec3::ZERO, &m);
        let sideways = cell_priority(Vec3::new(0.0, 0.0, 500.0), Vec3::ZERO, &m);
        assert!(on_path < sideways);
    }

    #[test]
    fn stationary_priority_is_plain_distance() {
        let m = CameraMotion::default();
        let d = cell_priority(Vec3::new(0.0, 0.0, 300.0), Vec3::ZERO, &m);
        assert!((d - 300.0).abs() < 0.001);
    }

    #[test]
    fn teleport_resets_velocity_instead_of_spiking_it() {
        let mut m = CameraMotion::default();
        m.sample(Vec3::ZERO, 0.016, 0.3, 1000.0);
        m.sample(Vec3::new(50_000.0, 0.0, 0.0), 0.016, 0.3, 1000.0);
        assert_eq!(m.speed, 0.0, "a teleport must not register as velocity");
        assert!(!m.is_moving());
    }

    #[test]
    fn smoothing_converges_toward_true_velocity() {
        let mut m = CameraMotion::default();
        let dt = 0.1;
        // 100 m/s along +X, sampled repeatedly.
        for i in 0..200 {
            m.sample(Vec3::new(10.0 * i as f32, 0.0, 0.0), dt, 0.3, 10_000.0);
        }
        assert!((m.speed - 100.0).abs() < 1.0, "speed {} should approach 100", m.speed);
        assert!(m.heading.dot(Vec3::X) > 0.99);
    }

    #[test]
    fn course_change_fires_only_on_a_real_turn() {
        let mut m = moving(Vec3::X, 50.0);
        m.mark_planned();
        assert!(!m.course_changed(0.85), "an unchanged heading is not a turn");
        m.heading = Vec3::new(1.0, 0.0, 0.15).normalize();
        assert!(!m.course_changed(0.85), "a small drift is not a turn");
        m.heading = -Vec3::X;
        assert!(m.course_changed(0.85), "a reversal IS a turn");
    }

    #[test]
    fn stopping_is_not_a_course_change() {
        let mut m = moving(Vec3::X, 50.0);
        m.mark_planned();
        m.speed = 0.0;
        m.heading = Vec3::ZERO;
        assert!(!m.course_changed(0.85));
    }

    #[test]
    fn near_ball_is_kept_regardless_of_heading() {
        let cam = Vec3::ZERO;
        let predicted = Vec3::new(1000.0, 0.0, 0.0);
        // Directly behind, but inside the near ball.
        assert!(cell_wanted(Vec3::new(-100.0, 0.0, 0.0), cam, predicted, 350.0, 200.0));
        // Behind and outside both regions.
        assert!(!cell_wanted(Vec3::new(-900.0, 0.0, 0.0), cam, predicted, 350.0, 200.0));
        // Far ahead but inside the prefetch corridor.
        assert!(cell_wanted(Vec3::new(1050.0, 0.0, 0.0), cam, predicted, 350.0, 200.0));
    }

    #[test]
    fn evict_radius_is_tighter_behind_than_ahead() {
        let m = moving(Vec3::X, 50.0);
        let cam = Vec3::ZERO;
        let front = directional_evict_radius(Vec3::new(100.0, 0.0, 0.0), cam, &m, 500.0, 300.0);
        let back = directional_evict_radius(Vec3::new(-100.0, 0.0, 0.0), cam, &m, 500.0, 300.0);
        assert_eq!(front, 500.0);
        assert_eq!(back, 300.0);
        // Stationary is symmetric again, so stopping never mass-evicts.
        let still = CameraMotion::default();
        assert_eq!(
            directional_evict_radius(Vec3::new(-100.0, 0.0, 0.0), cam, &still, 500.0, 300.0),
            500.0
        );
    }
}

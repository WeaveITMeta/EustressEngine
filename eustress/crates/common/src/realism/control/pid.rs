//! General-purpose PID controller with anti-windup, gain scheduling, feedforward.
//!
//! Usage:
//!   let mut pid = PidController::new(1.0, 0.1, 0.05)
//!       .with_limits(-100.0, 100.0)
//!       .with_anti_windup(50.0);
//!   let output = pid.update(setpoint, measured, dt);

/// A general-purpose PID controller with anti-windup, gain scheduling, and feedforward support.
///
/// Supports both derivative-on-error and derivative-on-measurement modes.
/// Anti-windup clamps the integral accumulator to prevent integrator saturation.
#[derive(Clone, Debug)]
pub struct PidController {
    pub kp: f32,
    pub ki: f32,
    pub kd: f32,
    pub setpoint: f32,
    pub output_min: f32,
    pub output_max: f32,
    /// Integral clamp expressed in output units. Integral is clamped to
    /// `[-anti_windup_limit / ki, anti_windup_limit / ki]` (when ki != 0).
    pub anti_windup_limit: f32,
    pub integral: f32,
    pub prev_error: f32,
    /// Used when `derivative_on_measurement` is true to compute D without
    /// reacting to setpoint step changes.
    pub prev_measured: f32,
    /// When true, the derivative term is `-kd * (measured - prev_measured) / dt`
    /// instead of `kd * (error - prev_error) / dt`, avoiding derivative kick on
    /// setpoint changes.
    pub derivative_on_measurement: bool,
    /// When false, `update()` returns 0 immediately.
    pub enabled: bool,
    /// Most recently computed output value (useful for monitoring / bumpless transfer).
    pub output: f32,
}

impl PidController {
    /// Create a new PID controller with the given gains.
    ///
    /// Defaults:
    /// - output limits: `[f32::NEG_INFINITY, f32::INFINITY]`
    /// - anti-windup limit: `f32::INFINITY` (disabled)
    /// - derivative on error
    /// - enabled
    pub fn new(kp: f32, ki: f32, kd: f32) -> Self {
        PidController {
            kp,
            ki,
            kd,
            setpoint: 0.0,
            output_min: f32::NEG_INFINITY,
            output_max: f32::INFINITY,
            anti_windup_limit: f32::INFINITY,
            integral: 0.0,
            prev_error: 0.0,
            prev_measured: 0.0,
            derivative_on_measurement: false,
            enabled: true,
            output: 0.0,
        }
    }

    /// Set symmetric or asymmetric output saturation limits.
    pub fn with_limits(mut self, min: f32, max: f32) -> Self {
        self.output_min = min;
        self.output_max = max;
        self
    }

    /// Clamp the integral accumulator so that `ki * integral` never exceeds
    /// `±anti_windup_limit` in output units.
    pub fn with_anti_windup(mut self, limit: f32) -> Self {
        self.anti_windup_limit = limit;
        self
    }

    /// Switch to derivative-on-measurement mode to avoid derivative kick when
    /// the setpoint changes suddenly.
    pub fn with_derivative_on_measurement(mut self) -> Self {
        self.derivative_on_measurement = true;
        self
    }

    /// Compute one PID step. Returns the clamped control output.
    ///
    /// Algorithm:
    /// ```text
    /// error = setpoint - measured
    /// P     = kp * error
    /// I    += ki * error * dt          (anti-windup clamped)
    /// D     = -kd * (measured - prev_measured) / dt   [derivative_on_measurement]
    ///       = kd  * (error - prev_error) / dt          [derivative_on_error]
    /// out   = clamp(P + I + D, output_min, output_max)
    /// ```
    pub fn update(&mut self, setpoint: f32, measured: f32, dt: f32) -> f32 {
        self.update_ff(setpoint, measured, 0.0, dt)
    }

    /// Compute one PID step with an additional feedforward term.
    ///
    /// `output = clamp(PID + feedforward, output_min, output_max)`
    pub fn update_ff(
        &mut self,
        setpoint: f32,
        measured: f32,
        feedforward: f32,
        dt: f32,
    ) -> f32 {
        if !self.enabled {
            return self.output;
        }

        // Guard against zero or negative dt to avoid division instability.
        let dt = dt.max(f32::EPSILON);

        self.setpoint = setpoint;
        let error = setpoint - measured;

        // Proportional term.
        let p = self.kp * error;

        // Integral term with anti-windup clamping.
        self.integral += self.ki * error * dt;
        if self.ki.abs() > f32::EPSILON {
            let limit = self.anti_windup_limit / self.ki.abs();
            self.integral = self.integral.clamp(-limit, limit);
        }
        let i = self.integral;

        // Derivative term.
        let d = if self.derivative_on_measurement {
            // Negate so that increasing measured value produces negative derivative output
            // (acts as a brake when approaching the setpoint from below).
            -self.kd * (measured - self.prev_measured) / dt
        } else {
            self.kd * (error - self.prev_error) / dt
        };

        // Update state for next step.
        self.prev_error = error;
        self.prev_measured = measured;

        let raw = p + i + d + feedforward;
        let clamped = raw.clamp(self.output_min, self.output_max);
        self.output = clamped;
        clamped
    }

    /// Reset the integrator, previous error, and previous measured value.
    ///
    /// Call this after a mode switch or extended pause to prevent accumulated
    /// state from producing a large initial output.
    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.prev_error = 0.0;
        self.prev_measured = 0.0;
        self.output = 0.0;
    }

    /// Update the setpoint without resetting the integrator.
    ///
    /// Use `reset()` first if bumpless transfer is not desired.
    pub fn set_setpoint(&mut self, sp: f32) {
        self.setpoint = sp;
    }

    /// Multiply the current gains by the supplied scale factors.
    ///
    /// Useful for gain scheduling without rebuilding the controller.
    /// Does **not** reset the integrator; call `reset()` if needed.
    pub fn scale_gains(&mut self, kp_scale: f32, ki_scale: f32, kd_scale: f32) {
        self.kp *= kp_scale;
        self.ki *= ki_scale;
        self.kd *= kd_scale;
    }
}

/// Gain-scheduled PID update.
///
/// Selects PID gains by linearly interpolating within `schedule`, a slice of
/// `(operating_point, kp, ki, kd)` tuples sorted in ascending `operating_point`
/// order.  The interpolated gains are applied to `pid` before calling `update()`.
///
/// If `operating_point` is below the first entry or above the last entry the
/// nearest boundary gains are used (i.e., no extrapolation).
///
/// Returns the controller output.
pub fn gain_scheduled_update(
    pid: &mut PidController,
    setpoint: f32,
    measured: f32,
    operating_point: f32,
    schedule: &[(f32, f32, f32, f32)],
    dt: f32,
) -> f32 {
    if schedule.is_empty() {
        return pid.update(setpoint, measured, dt);
    }

    // Find the bracketing pair.
    let (kp, ki, kd) = if operating_point <= schedule[0].0 {
        let e = &schedule[0];
        (e.1, e.2, e.3)
    } else if operating_point >= schedule[schedule.len() - 1].0 {
        let e = &schedule[schedule.len() - 1];
        (e.1, e.2, e.3)
    } else {
        // Binary search for the lower bracket index.
        let mut lo = 0usize;
        let mut hi = schedule.len() - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if schedule[mid].0 <= operating_point {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (op_lo, kp_lo, ki_lo, kd_lo) = schedule[lo];
        let (op_hi, kp_hi, ki_hi, kd_hi) = schedule[hi];
        let span = op_hi - op_lo;
        let t = if span.abs() > f32::EPSILON {
            (operating_point - op_lo) / span
        } else {
            0.0
        };
        let lerp = |a: f32, b: f32| a + t * (b - a);
        (lerp(kp_lo, kp_hi), lerp(ki_lo, ki_hi), lerp(kd_lo, kd_hi))
    };

    pid.kp = kp;
    pid.ki = ki;
    pid.kd = kd;

    pid.update(setpoint, measured, dt)
}

/// Cascade PID: the outer loop's output is fed as the setpoint for the inner loop.
///
/// Both loops are updated on the same `dt`.  In practice you may call the outer
/// loop less frequently, but this function advances both simultaneously.
///
/// Returns the inner-loop output (the actuator command).
pub fn cascade_update(
    outer: &mut PidController,
    inner: &mut PidController,
    outer_setpoint: f32,
    outer_measured: f32,
    inner_measured: f32,
    dt: f32,
) -> f32 {
    let inner_setpoint = outer.update(outer_setpoint, outer_measured, dt);
    inner.update(inner_setpoint, inner_measured, dt)
}

/// Bumpless transfer from manual to automatic control.
///
/// Pre-loads the integrator so that the first automatic output matches
/// `manual_output`, avoiding a step change in the control signal.
///
/// Call this immediately before switching the controller from manual to auto.
/// After calling this function `pid.enabled` should be set to `true`.
///
/// The pre-load satisfies:
/// ```text
/// manual_output = kp * (setpoint - measured) + integral + kd * 0
/// integral = manual_output - kp * (setpoint - measured)
/// ```
pub fn bumpless_transfer(pid: &mut PidController, manual_output: f32, measured: f32) {
    let error = pid.setpoint - measured;
    let p = pid.kp * error;
    pid.integral = manual_output - p;

    // Clamp the pre-loaded integral to the anti-windup limit.
    if pid.ki.abs() > f32::EPSILON {
        let limit = pid.anti_windup_limit / pid.ki.abs();
        pid.integral = pid.integral.clamp(-limit, limit);
    }

    // Reset derivative state so the first auto step has no derivative kick.
    pid.prev_error = error;
    pid.prev_measured = measured;
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.01;

    #[test]
    fn proportional_only() {
        let mut pid = PidController::new(2.0, 0.0, 0.0)
            .with_limits(-10.0, 10.0);
        let out = pid.update(5.0, 3.0, DT);
        // error = 2.0, P = 4.0
        assert!((out - 4.0).abs() < 1e-5, "expected 4.0, got {out}");
    }

    #[test]
    fn output_clamped() {
        let mut pid = PidController::new(100.0, 0.0, 0.0)
            .with_limits(-10.0, 10.0);
        let out = pid.update(5.0, 0.0, DT);
        assert!((out - 10.0).abs() < 1e-5, "expected 10.0 (clamped), got {out}");
    }

    #[test]
    fn integral_accumulates() {
        let mut pid = PidController::new(0.0, 1.0, 0.0);
        let mut out = 0.0;
        for _ in 0..100 {
            out = pid.update(1.0, 0.0, DT);
        }
        // integral = 1.0 * 1.0 * 0.01 * 100 = 1.0
        assert!((out - 1.0).abs() < 1e-4, "expected ~1.0, got {out}");
    }

    #[test]
    fn anti_windup_clamps_integral() {
        // ki=1, anti_windup=0.5 => integral clamped to ±0.5
        let mut pid = PidController::new(0.0, 1.0, 0.0)
            .with_anti_windup(0.5);
        for _ in 0..1000 {
            pid.update(1.0, 0.0, DT);
        }
        assert!(pid.integral.abs() <= 0.5 + 1e-5,
            "integral {} exceeded anti-windup limit", pid.integral);
    }

    #[test]
    fn derivative_on_measurement_no_kick() {
        let mut pid = PidController::new(0.0, 0.0, 1.0)
            .with_derivative_on_measurement();
        // First call: prev_measured = 0 by default; measured = 0 → D = 0
        let out1 = pid.update(0.0, 0.0, DT);
        // Setpoint step: measured unchanged, D should still be 0
        let out2 = pid.update(10.0, 0.0, DT);
        assert!(out1.abs() < 1e-5, "expected 0, got {out1}");
        assert!(out2.abs() < 1e-5, "no derivative kick expected, got {out2}");
    }

    #[test]
    fn reset_clears_state() {
        let mut pid = PidController::new(1.0, 1.0, 0.0);
        pid.update(1.0, 0.0, DT);
        pid.reset();
        assert_eq!(pid.integral, 0.0);
        assert_eq!(pid.prev_error, 0.0);
        assert_eq!(pid.output, 0.0);
    }

    #[test]
    fn disabled_returns_last_output() {
        let mut pid = PidController::new(1.0, 0.0, 0.0)
            .with_limits(-10.0, 10.0);
        pid.update(5.0, 0.0, DT); // output = 5.0
        pid.enabled = false;
        let out = pid.update(100.0, 0.0, DT);
        assert!((out - 5.0).abs() < 1e-5, "expected last output 5.0, got {out}");
    }

    #[test]
    fn feedforward_added() {
        let mut pid = PidController::new(1.0, 0.0, 0.0)
            .with_limits(-100.0, 100.0);
        let out = pid.update_ff(5.0, 3.0, 10.0, DT);
        // P = 2.0, ff = 10.0 → 12.0
        assert!((out - 12.0).abs() < 1e-5, "expected 12.0, got {out}");
    }

    #[test]
    fn bumpless_transfer_matches_manual() {
        let mut pid = PidController::new(2.0, 1.0, 0.0)
            .with_limits(-100.0, 100.0);
        pid.set_setpoint(10.0);
        let manual = 7.5_f32;
        let measured = 8.0_f32;
        bumpless_transfer(&mut pid, manual, measured);
        pid.enabled = true;
        // First auto step should reproduce manual_output (error = 2, P = 4, integral = 3.5)
        let out = pid.update(10.0, measured, DT);
        // With tiny dt the integral change is negligible; output ≈ manual_output
        assert!((out - manual).abs() < 0.1,
            "bumpless transfer: expected ~{manual}, got {out}");
    }

    #[test]
    fn gain_schedule_interpolates() {
        let schedule = [
            (0.0_f32,  1.0, 0.1, 0.01),
            (50.0_f32, 2.0, 0.2, 0.02),
            (100.0_f32,3.0, 0.3, 0.03),
        ];
        let mut pid = PidController::new(0.0, 0.0, 0.0)
            .with_limits(-1000.0, 1000.0);
        // At midpoint between 0 and 50 the gains should be interpolated.
        gain_scheduled_update(&mut pid, 1.0, 0.0, 25.0, &schedule, DT);
        assert!((pid.kp - 1.5).abs() < 1e-5, "kp interpolation failed: {}", pid.kp);
        assert!((pid.ki - 0.15).abs() < 1e-5, "ki interpolation failed: {}", pid.ki);
        assert!((pid.kd - 0.015).abs() < 1e-5, "kd interpolation failed: {}", pid.kd);
    }

    #[test]
    fn cascade_inner_follows_outer() {
        let mut outer = PidController::new(1.0, 0.0, 0.0)
            .with_limits(-10.0, 10.0);
        let mut inner = PidController::new(1.0, 0.0, 0.0)
            .with_limits(-10.0, 10.0);
        // outer setpoint=5, outer_measured=0 → outer output=5 (inner setpoint)
        // inner_measured=0 → inner output = 5
        let out = cascade_update(&mut outer, &mut inner, 5.0, 0.0, 0.0, DT);
        assert!((out - 5.0).abs() < 1e-5, "cascade output expected 5.0, got {out}");
    }

    #[test]
    fn scale_gains_multiplies() {
        let mut pid = PidController::new(1.0, 2.0, 3.0);
        pid.scale_gains(2.0, 0.5, 3.0);
        assert!((pid.kp - 2.0).abs() < 1e-6);
        assert!((pid.ki - 1.0).abs() < 1e-6);
        assert!((pid.kd - 9.0).abs() < 1e-6);
    }
}

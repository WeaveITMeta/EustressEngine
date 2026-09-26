//! # Humanoid state
//!
//! Roblox's `HumanoidStateType`, and the signals an `Animate` script listens
//! to, derived each frame from a character's movement: `Running(speed)`,
//! `Jumping(active)`, `FreeFalling(active)`, `Climbing(speed)` and
//! `StateChanged(old, new)`. `Humanoid:GetState()` reads the state from a
//! hidden property this writes.
//!
//! Speeds are metres per second, Eustress's unit.

use crate::datamodel::{DataModel, DmEvent, DmValue, EnumItem, InstanceId};

/// The hidden property `Humanoid:GetState()` reads.
pub const STATE_PROPERTY: &str = "__HumanoidState";

/// `Running` fires when the speed moves by more than this, and when it
/// reaches zero.
pub const RUNNING_EPSILON: f32 = 0.05;

/// Rising faster than this when leaving the ground is a jump; slower is a fall.
const TAKEOFF_SPEED: f32 = 0.5;

/// Roblox's `Enum.HumanoidStateType`, the states Eustress's characters use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HumanoidState {
    #[default]
    Running,
    Jumping,
    Freefall,
    Landed,
    Climbing,
    Seated,
    Swimming,
    Dead,
}

impl HumanoidState {
    pub fn name(self) -> &'static str {
        match self {
            HumanoidState::Running => "Running",
            HumanoidState::Jumping => "Jumping",
            HumanoidState::Freefall => "Freefall",
            HumanoidState::Landed => "Landed",
            HumanoidState::Climbing => "Climbing",
            HumanoidState::Seated => "Seated",
            HumanoidState::Swimming => "Swimming",
            HumanoidState::Dead => "Dead",
        }
    }

    pub fn enum_item(self) -> EnumItem {
        EnumItem::new("HumanoidStateType", self.name())
    }
}

/// One frame of a character's movement.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LocomotionSample {
    pub grounded: bool,
    /// Horizontal speed, m/s.
    pub planar_speed: f32,
    /// Vertical speed, m/s, upward positive.
    pub vertical_velocity: f32,
    pub climbing: bool,
    /// Speed along the wall while climbing, m/s.
    pub climb_speed: f32,
    pub seated: bool,
    pub swimming: bool,
    pub dead: bool,
}

/// One Humanoid's state from frame to frame.
#[derive(Debug, Clone, Default)]
pub struct HumanoidStateTracker {
    state: HumanoidState,
    last_speed: Option<f32>,
    last_climb: Option<f32>,
}

impl HumanoidStateTracker {
    pub fn state(&self) -> HumanoidState {
        self.state
    }

    /// Advance one frame. Returns the signals to fire on the Humanoid, in order.
    pub fn update(&mut self, s: LocomotionSample) -> Vec<(&'static str, Vec<DmValue>)> {
        let next = self.next_state(&s);
        let mut out: Vec<(&'static str, Vec<DmValue>)> = Vec::new();
        if next != self.state {
            let old = self.state;
            match old {
                HumanoidState::Jumping => out.push(("Jumping", vec![DmValue::Bool(false)])),
                HumanoidState::Freefall => out.push(("FreeFalling", vec![DmValue::Bool(false)])),
                _ => {}
            }
            match next {
                HumanoidState::Jumping => out.push(("Jumping", vec![DmValue::Bool(true)])),
                HumanoidState::Freefall => out.push(("FreeFalling", vec![DmValue::Bool(true)])),
                _ => {}
            }
            out.push(("StateChanged", vec![DmValue::Enum(old.enum_item()), DmValue::Enum(next.enum_item())]));
            self.state = next;
        }

        if matches!(self.state, HumanoidState::Running | HumanoidState::Landed) {
            let speed = if s.planar_speed.is_finite() { s.planar_speed.max(0.0) } else { 0.0 };
            let fire = match self.last_speed {
                None => true,
                Some(last) => {
                    (speed - last).abs() > RUNNING_EPSILON || (speed < RUNNING_EPSILON && last >= RUNNING_EPSILON)
                }
            };
            if fire {
                out.push(("Running", vec![DmValue::Number(speed as f64)]));
                self.last_speed = Some(speed);
            }
        } else {
            self.last_speed = None;
        }

        if self.state == HumanoidState::Climbing {
            let speed = if s.climb_speed.is_finite() { s.climb_speed } else { 0.0 };
            let fire = self.last_climb.map_or(true, |last| (speed - last).abs() > RUNNING_EPSILON);
            if fire {
                out.push(("Climbing", vec![DmValue::Number(speed as f64)]));
                self.last_climb = Some(speed);
            }
        } else {
            self.last_climb = None;
        }
        out
    }

    fn next_state(&self, s: &LocomotionSample) -> HumanoidState {
        use HumanoidState::*;
        if s.dead || self.state == Dead {
            return Dead;
        }
        if s.climbing {
            return Climbing;
        }
        if s.seated {
            return Seated;
        }
        if s.swimming {
            return Swimming;
        }
        if s.grounded {
            return match self.state {
                Jumping | Freefall => Landed,
                _ => Running,
            };
        }
        match self.state {
            // Jumping lasts the frame the character leaves the ground.
            Jumping | Freefall => Freefall,
            _ if s.vertical_velocity > TAKEOFF_SPEED => Jumping,
            _ => Freefall,
        }
    }
}

/// A Humanoid's movement this frame: writes its state for `GetState` and
/// fires its signals. The tree keeps each Humanoid's state from frame to
/// frame, so any character builder calls this once a frame and nothing more.
pub fn report_state(dm: &mut DataModel, humanoid: InstanceId, sample: LocomotionSample) {
    let mut tracker = dm.animation.humanoids.remove(&humanoid).unwrap_or_default();
    let signals = tracker.update(sample);
    let state = tracker.state();
    dm.animation.humanoids.insert(humanoid, tracker);
    dm.set_prop_from_engine(humanoid, STATE_PROPERTY, DmValue::String(state.name().into()));
    for (name, args) in signals {
        dm.push_event(DmEvent::Signal { id: humanoid, name: name.into(), args });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(speed: f32) -> LocomotionSample {
        LocomotionSample { grounded: true, planar_speed: speed, ..Default::default() }
    }

    fn names(signals: &[(&'static str, Vec<DmValue>)]) -> Vec<&'static str> {
        signals.iter().map(|(n, _)| *n).collect()
    }

    #[test]
    fn running_fires_on_change_and_at_zero_only() {
        let mut t = HumanoidStateTracker::default();
        assert_eq!(names(&t.update(ground(0.0))), vec!["Running"], "the first frame reports the speed");
        assert!(t.update(ground(0.01)).is_empty(), "a jitter below the threshold stays quiet");
        assert_eq!(names(&t.update(ground(1.4))), vec!["Running"]);
        assert!(t.update(ground(1.42)).is_empty());
        assert_eq!(names(&t.update(ground(0.0))), vec!["Running"], "stopping reports zero");
    }

    #[test]
    fn a_jump_goes_jumping_freefall_landed_running() {
        let mut t = HumanoidStateTracker::default();
        t.update(ground(2.0));
        let up = LocomotionSample { grounded: false, vertical_velocity: 5.0, planar_speed: 2.0, ..Default::default() };
        let s = t.update(up);
        assert_eq!(t.state(), HumanoidState::Jumping);
        assert!(names(&s).contains(&"Jumping") && names(&s).contains(&"StateChanged"));
        t.update(up);
        assert_eq!(t.state(), HumanoidState::Freefall);
        let s = t.update(ground(2.0));
        assert_eq!(t.state(), HumanoidState::Landed);
        assert!(names(&s).contains(&"FreeFalling"), "leaving free fall reports it");
        assert!(names(&s).contains(&"Running"), "landing reports the speed again");
        t.update(ground(2.0));
        assert_eq!(t.state(), HumanoidState::Running);
    }

    #[test]
    fn walking_off_a_ledge_is_a_fall_not_a_jump() {
        let mut t = HumanoidStateTracker::default();
        t.update(ground(1.0));
        t.update(LocomotionSample { grounded: false, vertical_velocity: -0.2, ..Default::default() });
        assert_eq!(t.state(), HumanoidState::Freefall);
    }

    #[test]
    fn death_is_final() {
        let mut t = HumanoidStateTracker::default();
        t.update(LocomotionSample { dead: true, ..Default::default() });
        assert_eq!(t.state(), HumanoidState::Dead);
        t.update(ground(3.0));
        assert_eq!(t.state(), HumanoidState::Dead);
    }

    #[test]
    fn the_tree_keeps_each_humanoids_state() {
        let mut dm = DataModel::new();
        let h = dm.create_virtual("Humanoid", "Humanoid", None);
        report_state(&mut dm, h, ground(2.0));
        let up = LocomotionSample { grounded: false, vertical_velocity: 5.0, ..Default::default() };
        report_state(&mut dm, h, up);
        assert_eq!(dm.get_prop(h, STATE_PROPERTY), Some(DmValue::String("Jumping".into())));
        report_state(&mut dm, h, up);
        assert_eq!(dm.get_prop(h, STATE_PROPERTY), Some(DmValue::String("Freefall".into())));
    }

    #[test]
    fn climbing_reports_its_speed() {
        let mut t = HumanoidStateTracker::default();
        let s = t.update(LocomotionSample { climbing: true, climb_speed: 0.8, ..Default::default() });
        assert_eq!(t.state(), HumanoidState::Climbing);
        assert!(names(&s).contains(&"Climbing"));
        assert!(t.update(LocomotionSample { climbing: true, climb_speed: 0.81, ..Default::default() }).is_empty());
    }
}

//! # Roblox pose easing
//!
//! A `Pose`'s `EasingStyle` and `EasingDirection` shape the segment from its
//! keyframe to the next keyframe that poses the same joint.
//!
//! `PoseEasingDirection` keeps Roblox's legacy meaning, which is the reverse
//! of TweenService's: a pose's `In` plays the curve TweenService calls `Out`,
//! and its `Out` plays TweenService's `In`. `CubicV2` is the corrected cubic.
//! The deprecated `Cubic` plays as Roblox's runtime plays it, with its
//! direction the other way round from `CubicV2`.

use bevy::prelude::*;

/// Roblox's `Enum.PoseEasingStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum PoseEasingStyle {
    #[default]
    Linear,
    Constant,
    Elastic,
    Cubic,
    Bounce,
    CubicV2,
}

impl PoseEasingStyle {
    pub fn from_name(name: &str) -> Option<Self> {
        let n = name.trim();
        let n = n.strip_prefix("Enum.").unwrap_or(n);
        let n = n.strip_prefix("PoseEasingStyle.").unwrap_or(n);
        Some(match n.to_ascii_lowercase().as_str() {
            "linear" => Self::Linear,
            "constant" => Self::Constant,
            "elastic" => Self::Elastic,
            "cubic" => Self::Cubic,
            "bounce" => Self::Bounce,
            "cubicv2" => Self::CubicV2,
            _ => return None,
        })
    }

    /// Roblox's numeric value, as rbxm files store it.
    pub fn from_roblox_value(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Linear,
            1 => Self::Constant,
            2 => Self::Elastic,
            3 => Self::Cubic,
            4 => Self::Bounce,
            5 => Self::CubicV2,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Constant => "Constant",
            Self::Elastic => "Elastic",
            Self::Cubic => "Cubic",
            Self::Bounce => "Bounce",
            Self::CubicV2 => "CubicV2",
        }
    }
}

/// Roblox's `Enum.PoseEasingDirection`, with its legacy meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum PoseEasingDirection {
    #[default]
    In,
    Out,
    InOut,
}

impl PoseEasingDirection {
    pub fn from_name(name: &str) -> Option<Self> {
        let n = name.trim();
        let n = n.strip_prefix("Enum.").unwrap_or(n);
        let n = n.strip_prefix("PoseEasingDirection.").unwrap_or(n);
        Some(match n.to_ascii_lowercase().as_str() {
            "in" => Self::In,
            "out" => Self::Out,
            "inout" => Self::InOut,
            _ => return None,
        })
    }

    pub fn from_roblox_value(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::In,
            1 => Self::Out,
            2 => Self::InOut,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::In => "In",
            Self::Out => "Out",
            Self::InOut => "InOut",
        }
    }
}

/// TweenService's sense of direction, which the curves below are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tween {
    In,
    Out,
    InOut,
}

/// How far along a segment the pose is, 0 at its keyframe and 1 at the next,
/// for a linear progress `t` in 0..=1. `Elastic` overshoots past 0 and 1.
pub fn ease(style: PoseEasingStyle, direction: PoseEasingDirection, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Pose directions are the reverse of TweenService's, except for the
    // deprecated `Cubic`, which Roblox's runtime plays the other way round.
    let reversed = match direction {
        PoseEasingDirection::In => Tween::Out,
        PoseEasingDirection::Out => Tween::In,
        PoseEasingDirection::InOut => Tween::InOut,
    };
    let tween = match style {
        PoseEasingStyle::Cubic => match reversed {
            Tween::In => Tween::Out,
            Tween::Out => Tween::In,
            Tween::InOut => Tween::InOut,
        },
        _ => reversed,
    };
    match style {
        PoseEasingStyle::Linear => t,
        PoseEasingStyle::Constant => match direction {
            // Holds the keyframe, snaps to the next one, or switches half way.
            PoseEasingDirection::In => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            PoseEasingDirection::Out => {
                if t > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            PoseEasingDirection::InOut => {
                if t < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
        },
        PoseEasingStyle::Cubic | PoseEasingStyle::CubicV2 => directional(tween, t, |x| x * x * x),
        PoseEasingStyle::Elastic => directional(tween, t, elastic_in),
        PoseEasingStyle::Bounce => directional(tween, t, bounce_in),
    }
}

/// A curve written as its `In` form, played in the given direction.
fn directional(tween: Tween, t: f32, ease_in: impl Fn(f32) -> f32) -> f32 {
    match tween {
        Tween::In => ease_in(t),
        Tween::Out => 1.0 - ease_in(1.0 - t),
        Tween::InOut => {
            if t < 0.5 {
                ease_in(2.0 * t) / 2.0
            } else {
                1.0 - ease_in(2.0 - 2.0 * t) / 2.0
            }
        }
    }
}

fn elastic_in(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let c4 = std::f32::consts::TAU / 3.0;
    -(2.0_f32.powf(10.0 * t - 10.0)) * ((10.0 * t - 10.75) * c4).sin()
}

fn bounce_out(t: f32) -> f32 {
    let n1 = 7.5625;
    let d1 = 2.75;
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

fn bounce_in(t: f32) -> f32 {
    1.0 - bounce_out(1.0 - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: [PoseEasingStyle; 6] = [
        PoseEasingStyle::Linear,
        PoseEasingStyle::Constant,
        PoseEasingStyle::Elastic,
        PoseEasingStyle::Cubic,
        PoseEasingStyle::Bounce,
        PoseEasingStyle::CubicV2,
    ];
    const DIRECTIONS: [PoseEasingDirection; 3] =
        [PoseEasingDirection::In, PoseEasingDirection::Out, PoseEasingDirection::InOut];

    #[test]
    fn every_curve_starts_at_its_keyframe_and_ends_at_the_next() {
        for style in STYLES {
            for direction in DIRECTIONS {
                let end = ease(style, direction, 1.0);
                assert!((end - 1.0).abs() < 1e-5, "{style:?} {direction:?} ends at {end}");
                if style != PoseEasingStyle::Constant || direction != PoseEasingDirection::Out {
                    let start = ease(style, direction, 0.0);
                    assert!(start.abs() < 1e-5, "{style:?} {direction:?} starts at {start}");
                }
            }
        }
    }

    #[test]
    fn pose_directions_are_the_reverse_of_tween_directions() {
        // CubicV2 `Out` is TweenService's cubic `In`: slow first.
        assert!(ease(PoseEasingStyle::CubicV2, PoseEasingDirection::Out, 0.25) < 0.25);
        // CubicV2 `In` is TweenService's cubic `Out`: fast first.
        assert!(ease(PoseEasingStyle::CubicV2, PoseEasingDirection::In, 0.25) > 0.25);
        // The deprecated Cubic runs the other way round from CubicV2.
        let v1 = ease(PoseEasingStyle::Cubic, PoseEasingDirection::In, 0.25);
        let v2 = ease(PoseEasingStyle::CubicV2, PoseEasingDirection::Out, 0.25);
        assert!((v1 - v2).abs() < 1e-6);
    }

    #[test]
    fn names_and_roblox_values_parse() {
        for (i, style) in STYLES.iter().enumerate() {
            assert_eq!(PoseEasingStyle::from_roblox_value(i as u32), Some(*style));
            assert_eq!(PoseEasingStyle::from_name(style.name()), Some(*style));
        }
        for (i, d) in DIRECTIONS.iter().enumerate() {
            assert_eq!(PoseEasingDirection::from_roblox_value(i as u32), Some(*d));
            assert_eq!(PoseEasingDirection::from_name(d.name()), Some(*d));
        }
        assert_eq!(PoseEasingStyle::from_name("Enum.PoseEasingStyle.Bounce"), Some(PoseEasingStyle::Bounce));
    }

    #[test]
    fn linear_is_the_identity() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((ease(PoseEasingStyle::Linear, PoseEasingDirection::In, t) - t).abs() < 1e-6);
        }
    }
}

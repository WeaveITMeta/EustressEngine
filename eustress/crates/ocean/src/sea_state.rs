//! Named sea states, for property text and presets.
//!
//! Each state is a wind speed U10 (m/s, 10 m above the water), roughly
//! Beaufort 0, 2, 4, 6, 7 and 9, and sets only the wind: swell, fetch and
//! the rest stay as they are. Fully developed, the significant height is
//! the Pierson-Moskowitz Hs = 0.21 U10^2 / g:
//!
//! | State  | U10 (m/s) | Hs at Earth gravity (m) |
//! |--------|-----------|-------------------------|
//! | Still  | 0         | 0                       |
//! | Calm   | 3         | 0.19                    |
//! | Breeze | 7         | 1.05                    |
//! | Choppy | 11        | 2.59                    |
//! | Rough  | 16        | 5.48                    |
//! | Storm  | 24        | 12.3                    |

use std::fmt;

/// A named wind for the sea.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SeaState {
    Still,
    Calm,
    #[default]
    Breeze,
    Choppy,
    Rough,
    Storm,
}

impl SeaState {
    pub const ALL: [SeaState; 6] =
        [SeaState::Still, SeaState::Calm, SeaState::Breeze, SeaState::Choppy, SeaState::Rough, SeaState::Storm];

    /// Wind speed U10 (m/s).
    pub fn wind_speed(self) -> f32 {
        match self {
            SeaState::Still => 0.0,
            SeaState::Calm => 3.0,
            SeaState::Breeze => 7.0,
            SeaState::Choppy => 11.0,
            SeaState::Rough => 16.0,
            SeaState::Storm => 24.0,
        }
    }

    /// The name used in property text.
    pub fn name(self) -> &'static str {
        match self {
            SeaState::Still => "Still",
            SeaState::Calm => "Calm",
            SeaState::Breeze => "Breeze",
            SeaState::Choppy => "Choppy",
            SeaState::Rough => "Rough",
            SeaState::Storm => "Storm",
        }
    }

    /// The state with this name, ignoring case and surrounding space.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name))
    }

    /// The fully developed significant height under gravity `gravity` (m).
    pub fn significant_wave_height(self, gravity: f32) -> f32 {
        pm_significant_wave_height(self.wind_speed(), gravity)
    }
}

impl fmt::Display for SeaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The Pierson-Moskowitz significant wave height 0.21 U10^2 / g (m) of a
/// fully developed sea. The spectrum integrates to 0.2093 U10^2 / g.
pub fn pm_significant_wave_height(u10: f32, gravity: f32) -> f32 {
    0.21 * u10 * u10 / gravity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::EARTH_GRAVITY;

    #[test]
    fn names_round_trip() {
        for s in SeaState::ALL {
            assert_eq!(SeaState::from_name(s.name()), Some(s));
            assert_eq!(SeaState::from_name(&s.to_string().to_uppercase()), Some(s));
            assert_eq!(SeaState::from_name(&format!("  {}\t", s.name().to_lowercase())), Some(s));
        }
        assert_eq!(SeaState::from_name("Hurricane"), None);
        assert_eq!(SeaState::from_name(""), None);
    }

    #[test]
    fn winds_rise_through_the_states() {
        let winds: Vec<f32> = SeaState::ALL.iter().map(|s| s.wind_speed()).collect();
        assert_eq!(winds, vec![0.0, 3.0, 7.0, 11.0, 16.0, 24.0]);
        assert_eq!(SeaState::default(), SeaState::Breeze);
    }

    #[test]
    fn pierson_moskowitz_height_follows_wind_and_gravity() {
        let hs = SeaState::Breeze.significant_wave_height(EARTH_GRAVITY);
        assert!((hs - 1.0493).abs() < 1e-3, "{hs}");
        assert_eq!(SeaState::Still.significant_wave_height(EARTH_GRAVITY), 0.0);
        let half_g = pm_significant_wave_height(11.0, EARTH_GRAVITY / 2.0);
        assert!((half_g / pm_significant_wave_height(11.0, EARTH_GRAVITY) - 2.0).abs() < 1e-6);
    }
}

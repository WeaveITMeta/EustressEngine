//! Which movement verbs an avatar may use.
//!
//! Everything is on by default. A Space turns verbs off for every player
//! through StarterPlayer properties (`JumpEnabled`, `ClimbingEnabled`, ...),
//! and a script turns them off for one character at runtime through the
//! Humanoid's properties of the same names or `Humanoid:SetStateEnabled`.
//! The avatar systems read this component where each verb begins, so turning
//! one off never interrupts a move already under way.

use bevy::prelude::*;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct AvatarAbilities {
    /// Jump from the ground.
    pub jump: bool,
    /// Run faster while sprint is held.
    pub sprint: bool,
    /// Grab ledges and hang, shimmy, turn corners, move between grips, mantle
    /// onto a top, and lower off an edge onto the ledge below.
    pub climb: bool,
    /// Stride over low obstacles instead of stopping at them.
    pub vault: bool,
    /// Leap across a gap from one grip to the next.
    pub ledge_leap: bool,
    /// Kick off a wall from a hang.
    pub wall_jump: bool,
    /// Roll out of a hard landing (a stumble otherwise).
    pub roll: bool,
}

impl Default for AvatarAbilities {
    fn default() -> Self {
        Self {
            jump: true,
            sprint: true,
            climb: true,
            vault: true,
            ledge_leap: true,
            wall_jump: true,
            roll: true,
        }
    }
}

impl AvatarAbilities {
    /// The property names shared by StarterPlayer (every player) and Humanoid
    /// (one character), with the field each one drives.
    pub const PROPERTIES: [&'static str; 7] = [
        "JumpEnabled",
        "SprintEnabled",
        "ClimbingEnabled",
        "VaultingEnabled",
        "LedgeLeapEnabled",
        "WallJumpEnabled",
        "RollEnabled",
    ];

    /// Set one ability by its property name. False for an unknown name.
    pub fn set(&mut self, property: &str, enabled: bool) -> bool {
        let slot = match property {
            "JumpEnabled" => &mut self.jump,
            "SprintEnabled" => &mut self.sprint,
            "ClimbingEnabled" => &mut self.climb,
            "VaultingEnabled" => &mut self.vault,
            "LedgeLeapEnabled" => &mut self.ledge_leap,
            "WallJumpEnabled" => &mut self.wall_jump,
            "RollEnabled" => &mut self.roll,
            _ => return false,
        };
        *slot = enabled;
        true
    }

    /// Read one ability by its property name.
    pub fn get(&self, property: &str) -> Option<bool> {
        Some(match property {
            "JumpEnabled" => self.jump,
            "SprintEnabled" => self.sprint,
            "ClimbingEnabled" => self.climb,
            "VaultingEnabled" => self.vault,
            "LedgeLeapEnabled" => self.ledge_leap,
            "WallJumpEnabled" => self.wall_jump,
            "RollEnabled" => self.roll,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_property_name_round_trips() {
        for name in AvatarAbilities::PROPERTIES {
            let mut a = AvatarAbilities::default();
            assert_eq!(a.get(name), Some(true), "{name}");
            assert!(a.set(name, false), "{name}");
            assert_eq!(a.get(name), Some(false), "{name}");
            // Only that one changed.
            let off = AvatarAbilities::PROPERTIES.iter().filter(|n| a.get(n) == Some(false)).count();
            assert_eq!(off, 1, "{name} turned off more than itself");
        }
        assert!(!AvatarAbilities::default().set("FlyingEnabled", false));
    }
}

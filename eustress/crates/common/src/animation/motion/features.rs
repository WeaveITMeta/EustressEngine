//! # Features
//!
//! The numbers a frame of a clip, or the character right now, is compared by.
//! All of them are in the character's frame (metres, root at the origin,
//! forward `-z`, right `+x`):
//!
//! | Group | Values |
//! |---|---|
//! | Trajectory position | `x`, `z` of the root at each trajectory time |
//! | Trajectory facing | the unit facing `x`, `z` at each trajectory time |
//! | Foot position | left and right foot, 3 each |
//! | Foot velocity | left and right foot, 3 each |
//! | Root velocity | `x`, vertical, `z` |
//!
//! With `n` trajectory times that is `4n + 15` values, 31 for the default four.
//! A group counts in the cost as the mean squared difference of its values, so
//! its weight, not the number of values in it, says how much it matters.

use std::ops::Range;

use bevy::math::Vec3;

use super::trajectory::TrajectoryPoint;

/// Values of the groups that do not depend on the trajectory.
pub const FOOT_VALUES: usize = 6;
pub const ROOT_VELOCITY_VALUES: usize = 3;

/// The groups of values, in the order they are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    TrajectoryPosition,
    TrajectoryFacing,
    FootPosition,
    FootVelocity,
    RootVelocity,
}

impl Group {
    pub const ALL: [Group; 5] =
        [Group::TrajectoryPosition, Group::TrajectoryFacing, Group::FootPosition, Group::FootVelocity, Group::RootVelocity];

    pub const fn index(self) -> usize {
        match self {
            Group::TrajectoryPosition => 0,
            Group::TrajectoryFacing => 1,
            Group::FootPosition => 2,
            Group::FootVelocity => 3,
            Group::RootVelocity => 4,
        }
    }
}

/// How many values a feature row has, by the number of trajectory times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureLayout {
    pub trajectory_samples: usize,
}

impl FeatureLayout {
    pub const fn new(trajectory_samples: usize) -> Self {
        Self { trajectory_samples }
    }

    /// Values in one row.
    pub const fn dims(self) -> usize {
        4 * self.trajectory_samples + 2 * FOOT_VALUES + ROOT_VELOCITY_VALUES
    }

    /// Where a group's values sit in a row.
    pub fn range(self, group: Group) -> Range<usize> {
        let n = self.trajectory_samples;
        let (start, len) = match group {
            Group::TrajectoryPosition => (0, 2 * n),
            Group::TrajectoryFacing => (2 * n, 2 * n),
            Group::FootPosition => (4 * n, FOOT_VALUES),
            Group::FootVelocity => (4 * n + FOOT_VALUES, FOOT_VALUES),
            Group::RootVelocity => (4 * n + 2 * FOOT_VALUES, ROOT_VELOCITY_VALUES),
        };
        start..start + len
    }

    /// The group a value belongs to.
    pub fn group_of(self, dim: usize) -> Group {
        Group::ALL.into_iter().find(|g| self.range(*g).contains(&dim)).unwrap_or(Group::RootVelocity)
    }
}

/// How much each group counts in the cost.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureWeights {
    pub trajectory_position: f32,
    pub trajectory_facing: f32,
    pub foot_position: f32,
    pub foot_velocity: f32,
    pub root_velocity: f32,
}

impl Default for FeatureWeights {
    fn default() -> Self {
        Self {
            trajectory_position: 1.0,
            trajectory_facing: 1.0,
            foot_position: 0.7,
            foot_velocity: 0.5,
            root_velocity: 0.5,
        }
    }
}

impl FeatureWeights {
    /// The four weights a `MotionDatabase` exposes: the velocity weight covers
    /// the feet's and the root's speeds, and the pose weight the feet's places.
    pub fn from_class(trajectory: f32, facing: f32, pose: f32, velocity: f32) -> Self {
        Self {
            trajectory_position: trajectory,
            trajectory_facing: facing,
            foot_position: pose,
            foot_velocity: velocity,
            root_velocity: velocity,
        }
    }

    pub fn of(&self, group: Group) -> f32 {
        match group {
            Group::TrajectoryPosition => self.trajectory_position,
            Group::TrajectoryFacing => self.trajectory_facing,
            Group::FootPosition => self.foot_position,
            Group::FootVelocity => self.foot_velocity,
            Group::RootVelocity => self.root_velocity,
        }
        .max(0.0)
    }
}

/// What the character is doing right now, as the matcher asks the database
/// about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    /// The predicted path, one point per trajectory time.
    pub trajectory: Vec<TrajectoryPoint>,
    pub left_foot: Vec3,
    pub right_foot: Vec3,
    pub left_foot_velocity: Vec3,
    pub right_foot_velocity: Vec3,
    /// `x`, vertical, `z`.
    pub root_velocity: Vec3,
}

impl Query {
    /// Write the raw values of this query into `out` in `layout`. `false`, with
    /// `out` untouched, when the path has the wrong number of points.
    pub fn write(&self, layout: FeatureLayout, out: &mut Vec<f32>) -> bool {
        if self.trajectory.len() != layout.trajectory_samples {
            return false;
        }
        out.clear();
        out.extend(self.trajectory.iter().flat_map(|p| [p.position.x, p.position.y]));
        out.extend(self.trajectory.iter().flat_map(|p| [p.facing.x, p.facing.y]));
        out.extend(self.left_foot.to_array());
        out.extend(self.right_foot.to_array());
        out.extend(self.left_foot_velocity.to_array());
        out.extend(self.right_foot_velocity.to_array());
        out.extend(self.root_velocity.to_array());
        debug_assert_eq!(out.len(), layout.dims());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    #[test]
    fn four_trajectory_times_make_thirty_one_values() {
        let layout = FeatureLayout::new(4);
        assert_eq!(layout.dims(), 31);
        let mut covered = 0;
        let mut next = 0;
        for group in Group::ALL {
            let range = layout.range(group);
            assert_eq!(range.start, next, "{group:?} follows the group before it");
            next = range.end;
            covered += range.len();
        }
        assert_eq!(covered, layout.dims());
        assert_eq!(layout.group_of(0), Group::TrajectoryPosition);
        assert_eq!(layout.group_of(8), Group::TrajectoryFacing);
        assert_eq!(layout.group_of(30), Group::RootVelocity);
    }

    #[test]
    fn a_query_writes_its_groups_where_the_layout_says() {
        let layout = FeatureLayout::new(2);
        let query = Query {
            trajectory: vec![
                TrajectoryPoint { position: Vec2::new(1.0, 2.0), facing: Vec2::new(3.0, 4.0) },
                TrajectoryPoint { position: Vec2::new(5.0, 6.0), facing: Vec2::new(7.0, 8.0) },
            ],
            left_foot: Vec3::new(10.0, 11.0, 12.0),
            right_foot: Vec3::new(13.0, 14.0, 15.0),
            left_foot_velocity: Vec3::new(16.0, 17.0, 18.0),
            right_foot_velocity: Vec3::new(19.0, 20.0, 21.0),
            root_velocity: Vec3::new(22.0, 23.0, 24.0),
        };
        let mut row = Vec::new();
        assert!(query.write(layout, &mut row));
        assert_eq!(row.len(), layout.dims());
        assert_eq!(&row[layout.range(Group::TrajectoryPosition)], &[1.0, 2.0, 5.0, 6.0]);
        assert_eq!(&row[layout.range(Group::TrajectoryFacing)], &[3.0, 4.0, 7.0, 8.0]);
        assert_eq!(&row[layout.range(Group::FootPosition)], &[10.0, 11.0, 12.0, 13.0, 14.0, 15.0]);
        assert_eq!(&row[layout.range(Group::FootVelocity)], &[16.0, 17.0, 18.0, 19.0, 20.0, 21.0]);
        assert_eq!(&row[layout.range(Group::RootVelocity)], &[22.0, 23.0, 24.0]);
    }

    #[test]
    fn a_query_with_the_wrong_path_length_writes_nothing() {
        let mut row = vec![9.0];
        assert!(!Query::default().write(FeatureLayout::new(4), &mut row));
        assert_eq!(row, vec![9.0]);
    }

    #[test]
    fn weights_never_go_negative() {
        let weights = FeatureWeights { foot_position: -1.0, ..Default::default() };
        assert_eq!(weights.of(Group::FootPosition), 0.0);
        let from = FeatureWeights::from_class(1.0, 2.0, 3.0, 4.0);
        assert_eq!(from.of(Group::FootVelocity), 4.0);
        assert_eq!(from.of(Group::RootVelocity), 4.0);
        assert_eq!(from.of(Group::FootPosition), 3.0);
    }
}

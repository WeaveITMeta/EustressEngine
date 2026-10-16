//! # Motion matching: the core
//!
//! The parts of motion matching that need no scene
//! (`docs/design/MOTION_MATCHING.md`). Each file but [`sample`] is plain data and
//! arithmetic, with no ECS or asset access, so it is tested on synthetic clips:
//!
//! - [`trajectory`]: where a character is about to go and face, predicted with
//!   the spring the controller follows.
//! - [`features`]: the numbers a frame is compared by, and how much each group
//!   counts.
//! - [`database`]: every frame of every clip as a row of features, with the feet
//!   that are planted on it.
//! - [`search`]: the matcher that plays a clip and looks through the rows for a
//!   better one, without thrashing.
//! - [`inertialize`]: the decaying offset that hides a switch.
//! - [`sample`]: the frames of a Bevy clip, read at a fixed rate through a forward
//!   pass over the skeleton, as the database's input. It reads a clip asset and
//!   nothing else.
//! - `shadow` (with the `physics` feature): a matcher that watches a character
//!   and reports the clip it would pick, beside the `Animate` script's gait.

pub mod database;
pub mod features;
pub mod inertialize;
pub mod sample;
pub mod search;
#[cfg(feature = "physics")]
pub mod shadow;
pub mod trajectory;

pub use database::{ClipFrames, ClipMeta, Contacts, Database, FrameInfo, PoseFrame, RootMotion};
pub use features::{FeatureLayout, FeatureWeights, Group, Query};
pub use inertialize::{Inertializer, JointVel, JointXf};
pub use sample::{ClipSpec, HipsMode, Landmarks, Reference, ShapeJoint, SkeletonShape};
pub use search::{MatchStep, Matcher, SearchConfig, TagFilter};
pub use trajectory::{predict_trajectory, Desired, PlanarState, TrajectoryPoint, DEFAULT_TRAJECTORY_TIMES};

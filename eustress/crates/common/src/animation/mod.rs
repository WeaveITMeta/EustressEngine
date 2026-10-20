//! # Animation
//!
//! Animation built from instances, with Roblox's semantics: an `Animation`
//! names a clip, an `Animator` plays clips on its rig, and
//! `Animator:LoadAnimation` returns an `AnimationTrack`. The tracks live in
//! the tree ([`crate::datamodel::animation`]); this module is the pose
//! evaluator both shells add, and the local character both shells share.
//! The design is `docs/design/ANIMATION_SYSTEM.md`.
//!
//! ## What a shell does
//!
//! 1. Insert [`LiveTree`] with the tree it animates from: Studio at Play
//!    start beside its Play tree, the Player beside its own tree.
//! 2. Add [`AnimatorPlugin`].
//! 3. Place [`AnimatorSet::Step`] after its clock advances and before its
//!    scripts, and [`AnimatorSet::Drive`] after its scripts.
//! 4. Run [`character::sync_local_character`] where it pulls engine state
//!    into the tree, before its scripts.
//!
//! ## Contracts that compile in every build
//!
//! [`AnimatorSet`] and [`PoseReady`] are defined here without a feature
//! gate. Systems elsewhere order themselves against the sets and read the
//! marker, so no module names a gated one.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::datamodel::SharedDataModel;

pub mod animate;
pub mod character;
pub mod clip;
pub mod content;
pub mod easing;
pub mod graph;
pub mod humanoid;
pub mod runtime;
pub mod tree;

pub use clip::JointPose;
pub use runtime::{AnimatorIndex, AnimatorRuntime};
pub use tree::{materialize_sequence, sequence_from_tree};

/// The tree this shell animates from, and the Space folder its clips live in.
#[derive(Resource, Clone)]
pub struct LiveTree {
    pub dm: SharedDataModel,
    pub space_root: Option<PathBuf>,
}

/// The phases of one animated frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatorSet {
    /// `Update`, before scripts: tracks advance to the clock and raise their
    /// events.
    Step,
    /// `Update`, after scripts: rigs bind, clips load, graphs rebuild, and
    /// this frame's weights and times go to each rig's player.
    Drive,
    /// `PostUpdate`, after Bevy samples the graphs: `RootMotionMode` `Pin`.
    RootMotion,
    /// Breathing, exertion, idle breaks, landing flex, look-at: composed onto
    /// the animated pose, never assigned.
    Procedural,
    /// `IKControl`, foot placement, climb grips.
    Ik,
    /// The avatar's facing.
    Facing,
    /// Script writes to a joint's `Transform`.
    Overrides,
    /// The `Motor6D` forward pass.
    Joints,
}

/// A rig whose pose is final for this frame's measurements: the animated
/// pose once its Animator plays, or the rest pose when nothing will animate
/// it. The avatar's feet calibrate against it.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PoseReady;

/// An avatar an Animator has taken over, once one of its tracks played. From
/// then on its bones answer to the Animator, and the avatar's own motion graph
/// (`avatar::anim`), which animates every other avatar, lets it go.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct AnimatorDriven;

/// With `EUSTRESS_LEGACY_MOTION=1` no Animator binds an avatar, so the
/// avatar's own motion graph animates every one: the reference the
/// Animator's gait is compared against.
pub fn legacy_motion() -> bool {
    std::env::var("EUSTRESS_LEGACY_MOTION").is_ok_and(|v| v == "1")
}

/// The pose evaluator. Needs Bevy's `AnimationPlugin`.
pub struct AnimatorPlugin;

impl Plugin for AnimatorPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<JointPose>().init_resource::<AnimatorIndex>();

        app.configure_sets(Update, AnimatorSet::Step.before(AnimatorSet::Drive));
        app.configure_sets(
            PostUpdate,
            (
                AnimatorSet::RootMotion,
                AnimatorSet::Procedural,
                AnimatorSet::Ik,
                AnimatorSet::Facing,
                AnimatorSet::Overrides,
                AnimatorSet::Joints,
            )
                .chain()
                .after(bevy::app::AnimationSystems)
                .before(TransformSystems::Propagate),
        );

        app.add_systems(Update, runtime::step_tracks.in_set(AnimatorSet::Step)).add_systems(
            Update,
            (
                runtime::capture_bind_poses,
                runtime::track_animators,
                runtime::bind_rigs,
                runtime::load_clips,
                runtime::drive_graphs,
                runtime::pose_ready_fallback,
            )
                .chain()
                .in_set(AnimatorSet::Drive),
        );
        app.add_systems(
            PostUpdate,
            (
                runtime::pin_root_motion.in_set(AnimatorSet::RootMotion),
                runtime::motor_forward_pass.in_set(AnimatorSet::Joints),
                runtime::probe_liveness.in_set(AnimatorSet::Joints).after(runtime::motor_forward_pass),
            ),
        );
    }
}

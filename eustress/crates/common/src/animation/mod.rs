//! # Animation
//!
//! Animation built from instances, with Roblox's semantics: an `Animation`
//! names a clip, an `Animator` plays clips on its rig, and
//! `Animator:LoadAnimation` returns an `AnimationTrack`. The tracks live in
//! the tree ([`crate::datamodel::animation`]); this module is the pose
//! evaluator both shells add, and the local character both shells share.
//! The design is `docs/design/ANIMATION_SYSTEM.md`.
//!
//! ## What an app does
//!
//! Add [`AnimatorPlugin`]. It animates from the Play session's tree
//! ([`crate::play_session::PlayDataModel`]), reads clips from the open
//! Space's folder ([`crate::play_session::PlayAssetRoot`]), and orders its
//! sets against [`crate::play_session::PlayScriptSet`]: tracks step after
//! `Pull` and before `Scripts`, and rigs are driven after `Apply` and before
//! `End`. An app whose own code does not build the local character runs
//! [`character::sync_local_character`] before `Scripts`.
//!
//! ## Contracts that compile in every build
//!
//! [`AnimatorSet`] and [`PoseReady`] are defined here without a feature
//! gate. Systems elsewhere order themselves against the sets and read the
//! marker, so no module names a gated one.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::datamodel::SharedDataModel;
use crate::play_session::{PlayAssetRoot, PlayDataModel, PlayScriptSet};

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

/// The tree this app animates from, and the Space folder its clips live in:
/// the Play session's, kept by [`follow_play_session`].
#[derive(Resource, Clone)]
pub struct LiveTree {
    pub dm: SharedDataModel,
    pub space_root: Option<PathBuf>,
}

/// [`LiveTree`] is the Play session's tree with the open Space's folder,
/// and there is none outside a session.
pub fn follow_play_session(
    mut commands: Commands,
    session: Option<Res<PlayDataModel>>,
    root: Option<Res<PlayAssetRoot>>,
    live: Option<Res<LiveTree>>,
) {
    match session {
        Some(session) => {
            let space_root = root.map(|r| r.0.clone()).filter(|p| !p.as_os_str().is_empty());
            let current =
                live.is_some_and(|l| std::sync::Arc::ptr_eq(&l.dm, &session.dm) && l.space_root == space_root);
            if !current {
                commands.insert_resource(LiveTree { dm: session.dm.clone(), space_root });
            }
        }
        None => {
            if live.is_some() {
                commands.remove_resource::<LiveTree>();
            }
        }
    }
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

        app.configure_sets(
            Update,
            (
                AnimatorSet::Step.after(PlayScriptSet::Pull).before(PlayScriptSet::Scripts),
                AnimatorSet::Drive.after(PlayScriptSet::Apply).before(PlayScriptSet::End),
            ),
        );
        app.add_systems(Update, follow_play_session.before(PlayScriptSet::Pull));
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

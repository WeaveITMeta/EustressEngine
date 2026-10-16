//! # Frames from a Bevy clip
//!
//! The bridge between a clip the engine plays and the [`database`](super::database)
//! the matcher searches. It reads a Bevy [`AnimationClip`] at a fixed frame
//! rate, runs a forward pass over the skeleton, and gives the hips and feet in
//! the character's own frame, as [`ClipFrames`].
//!
//! Three steps, each usable alone:
//!
//! 1. [`sample_locals`]: every joint's local transform at one time. The clip's
//!    curves supply the translation, rotation and scale it animates; a joint or
//!    a channel the clip leaves out keeps its rest value. Only curves on a
//!    `Transform` are read, which is what a skeleton's clips use.
//! 2. [`forward_pass`]: the skeleton's joints placed in the frame above its
//!    root, which includes the root's own transform: the armature's scale and
//!    the clip's up-axis correction. Positions come out in metres.
//! 3. [`pose_frames`]: those positions measured the way the matcher measures a
//!    character: `x` and `z` from the hips at rest, `y` from the soles at rest,
//!    turned so forward is `-z`.
//!
//! **Pinned hips.** The runtime pins the hips' translation to the bind pose and
//! lets the controller move the body, so the live feet are posed under hips that
//! do not move. [`HipsMode::PinnedToRest`] samples a clip the same way, so a
//! frame of the database is the pose the screen would show. [`HipsMode::Authored`]
//! keeps the clip's own hip motion, for a clip whose vertical motion is the
//! point (a jump).
//!
//! The module reads a clip and a description of the skeleton and touches no
//! scene, so the caller builds the [`SkeletonShape`] from a live rig and says
//! which joints are the landmarks.

use std::any::TypeId;

use bevy::animation::animation_curves::EvaluatorId;
use bevy::animation::{AnimationClip, AnimationTargetId};
use bevy::math::{Mat4, Quat, Vec3};
use bevy::prelude::Transform;

use super::database::{ClipFrames, PoseFrame};

/// Field positions in `Transform`, as Bevy's reflection numbers them.
const FIELD_TRANSLATION: usize = 0;
const FIELD_ROTATION: usize = 1;
const FIELD_SCALE: usize = 2;

/// One joint of a skeleton.
#[derive(Debug, Clone)]
pub struct ShapeJoint {
    /// What the clip's curves are keyed by.
    pub target: AnimationTargetId,
    /// Index of the parent joint, which must come before this one.
    pub parent: Option<usize>,
    /// The joint's local transform at rest.
    pub rest: Transform,
}

/// A skeleton as the sampler needs it.
#[derive(Debug, Clone)]
pub struct SkeletonShape {
    /// Parents before children.
    pub joints: Vec<ShapeJoint>,
    /// The transform above the first joints: the armature's scale and the clip's
    /// up-axis correction.
    pub root: Transform,
}

impl SkeletonShape {
    /// `Err` when a joint's parent does not come before it.
    pub fn validate(&self) -> Result<(), String> {
        if self.joints.is_empty() {
            return Err("a skeleton shape needs at least one joint".into());
        }
        for (i, joint) in self.joints.iter().enumerate() {
            if let Some(parent) = joint.parent {
                if parent >= i {
                    return Err(format!("joint {i} has parent {parent}, which does not come before it"));
                }
            }
        }
        Ok(())
    }
}

/// The joints the matcher measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landmarks {
    pub hips: usize,
    pub left_foot: usize,
    pub right_foot: usize,
}

impl Landmarks {
    fn validate(&self, joints: usize) -> Result<(), String> {
        for (name, index) in [("hips", self.hips), ("left foot", self.left_foot), ("right foot", self.right_foot)] {
            if index >= joints {
                return Err(format!("the {name} is joint {index} of {joints}"));
            }
        }
        Ok(())
    }
}

/// Whether the clip's own motion of the hips is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HipsMode {
    /// Hold the hips' translation at its rest value, as the runtime does.
    #[default]
    PinnedToRest,
    /// Keep the hips' translation the clip gives.
    Authored,
}

/// Where the hips and feet are on one frame, in the frame above the skeleton
/// root, metres.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RawFrame {
    pub hips: Vec3,
    pub left_foot: Vec3,
    pub right_foot: Vec3,
}

/// Every joint's local transform at `time` seconds into `clip`, into `out`.
/// Times past the ends of a curve hold its end value.
pub fn sample_locals(clip: &AnimationClip, shape: &SkeletonShape, time: f32, out: &mut Vec<Transform>) {
    out.clear();
    for joint in &shape.joints {
        let mut local = joint.rest;
        if let Some(curves) = clip.curves_for_target(joint.target) {
            for curve in curves {
                let EvaluatorId::ComponentField(field) = curve.0.evaluator_id() else {
                    continue;
                };
                let (component, index) = **field;
                if component != TypeId::of::<Transform>() {
                    continue;
                }
                let value = curve.0.sample_clamped(time);
                match index {
                    FIELD_TRANSLATION => {
                        if let Some(v) = value.downcast_ref::<Vec3>() {
                            local.translation = *v;
                        }
                    }
                    FIELD_ROTATION => {
                        if let Some(q) = value.downcast_ref::<Quat>() {
                            local.rotation = *q;
                        }
                    }
                    FIELD_SCALE => {
                        if let Some(v) = value.downcast_ref::<Vec3>() {
                            local.scale = *v;
                        }
                    }
                    _ => {}
                }
            }
        }
        out.push(local);
    }
}

/// Every joint's placement in the frame above the skeleton root, into `worlds`.
/// `locals` is one transform per joint, as [`sample_locals`] gives.
pub fn forward_pass(shape: &SkeletonShape, locals: &[Transform], worlds: &mut Vec<Mat4>) {
    worlds.clear();
    let root = shape.root.to_matrix();
    for (i, joint) in shape.joints.iter().enumerate() {
        let parent = joint.parent.map_or(root, |p| worlds[p]);
        let local = locals.get(i).copied().unwrap_or(joint.rest);
        worlds.push(parent * local.to_matrix());
    }
}

fn position(worlds: &[Mat4], joint: usize) -> Vec3 {
    worlds[joint].w_axis.truncate()
}

/// What the frames of a clip are measured against: the skeleton at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reference {
    /// The hips at rest; `x` and `z` of every point are measured from it.
    pub hips: Vec3,
    /// The height of the lower sole at rest; `y` is measured from it.
    pub ground: f32,
    /// Turns the skeleton's frame into the character's, in which forward is
    /// `-z` and right is `+x`. It turns about the vertical axis through the hips.
    pub frame: Quat,
}

/// The reference of a skeleton standing at rest, turned by `frame`.
pub fn reference_from_rest(shape: &SkeletonShape, landmarks: Landmarks, frame: Quat) -> Result<Reference, String> {
    shape.validate()?;
    landmarks.validate(shape.joints.len())?;
    let locals: Vec<Transform> = shape.joints.iter().map(|j| j.rest).collect();
    let mut worlds = Vec::new();
    forward_pass(shape, &locals, &mut worlds);
    let left = position(&worlds, landmarks.left_foot);
    let right = position(&worlds, landmarks.right_foot);
    Ok(Reference { hips: position(&worlds, landmarks.hips), ground: left.y.min(right.y), frame })
}

/// The hips and feet of `clip` at `fps` frames a second. A looping clip ends
/// where it starts, so its last frame is not a repeat of its first.
pub fn sample_frames(
    clip: &AnimationClip,
    shape: &SkeletonShape,
    landmarks: Landmarks,
    fps: f32,
    looping: bool,
    hips: HipsMode,
) -> Result<Vec<RawFrame>, String> {
    shape.validate()?;
    landmarks.validate(shape.joints.len())?;
    if !(fps.is_finite() && fps > 0.0) {
        return Err(format!("a frame rate of {fps}"));
    }
    let duration = clip.duration();
    if !(duration.is_finite() && duration > 0.0) {
        return Err("the clip has no length".into());
    }
    let count = if looping { (duration * fps).round() as usize } else { (duration * fps).floor() as usize + 1 };
    if count < 2 {
        return Err(format!("{count} frame(s) at {fps} fps, need at least 2"));
    }
    let hips_rest = shape.joints[landmarks.hips].rest.translation;
    let mut locals = Vec::with_capacity(shape.joints.len());
    let mut worlds = Vec::with_capacity(shape.joints.len());
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let time = (i as f32 / fps).min(duration);
        sample_locals(clip, shape, time, &mut locals);
        if hips == HipsMode::PinnedToRest {
            locals[landmarks.hips].translation = hips_rest;
        }
        forward_pass(shape, &locals, &mut worlds);
        frames.push(RawFrame {
            hips: position(&worlds, landmarks.hips),
            left_foot: position(&worlds, landmarks.left_foot),
            right_foot: position(&worlds, landmarks.right_foot),
        });
    }
    Ok(frames)
}

impl Reference {
    /// A point in the frame above the skeleton root, measured the way the
    /// matcher measures a character: `x` and `z` from the hips at rest, `y`
    /// from the soles at rest, turned so forward is `-z`. A live character's
    /// bones are measured with this too, so a stored frame and a live one are
    /// comparable.
    pub fn measure(&self, point: Vec3) -> Vec3 {
        self.frame * (point - Vec3::new(self.hips.x, self.ground, self.hips.z))
    }
}

/// Raw frames measured against a reference, as the matcher measures a character.
pub fn pose_frames(raw: &[RawFrame], reference: &Reference) -> Vec<PoseFrame> {
    raw.iter()
        .map(|f| PoseFrame {
            left_foot: reference.measure(f.left_foot),
            right_foot: reference.measure(f.right_foot),
            hips_height: reference.measure(f.hips).y,
        })
        .collect()
}

/// What a database clip is called and how its root moves, which the clip's
/// curves do not say. The values come from the `Animation`'s attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipSpec {
    pub name: String,
    pub tags: Vec<String>,
    pub fps: f32,
    pub looping: bool,
    pub hips: HipsMode,
    /// Root speed in m/s, for a clip that is in place.
    pub speed: f32,
    /// Direction of travel against the facing, degrees: 0 forward, 90 right.
    pub heading_degrees: f32,
    /// Turning rate, degrees a second, positive to the left.
    pub yaw_rate_degrees: f32,
}

impl Default for ClipSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            tags: Vec::new(),
            fps: 30.0,
            looping: true,
            hips: HipsMode::PinnedToRest,
            speed: 0.0,
            heading_degrees: 0.0,
            yaw_rate_degrees: 0.0,
        }
    }
}

/// A clip as the database builds from it.
pub fn clip_frames(
    clip: &AnimationClip,
    shape: &SkeletonShape,
    landmarks: Landmarks,
    reference: &Reference,
    spec: &ClipSpec,
) -> Result<ClipFrames, String> {
    let raw = sample_frames(clip, shape, landmarks, spec.fps, spec.looping, spec.hips)
        .map_err(|e| format!("clip {:?}: {e}", spec.name))?;
    Ok(ClipFrames {
        name: spec.name.clone(),
        fps: spec.fps,
        looping: spec.looping,
        tags: spec.tags.clone(),
        frames: pose_frames(&raw, reference),
        root: Vec::new(),
    }
    .with_constant_root(spec.speed, spec.heading_degrees, spec.yaw_rate_degrees))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::clip::skeleton_target;
    use bevy::animation::animated_field;
    use bevy::animation::animation_curves::{AnimatableCurve, AnimatedField};
    use bevy::math::curve::UnevenSampleAutoCurve;

    const HIPS: usize = 0;
    const LEFT: usize = 1;
    const RIGHT: usize = 2;

    fn joint(key: &str, parent: Option<usize>, translation: Vec3) -> ShapeJoint {
        ShapeJoint { target: skeleton_target(key), parent, rest: Transform::from_translation(translation) }
    }

    /// Hips 1 m up, a foot under each hip, 10 cm to either side.
    fn shape() -> SkeletonShape {
        SkeletonShape {
            joints: vec![
                joint("hips", None, Vec3::new(0.0, 1.0, 0.0)),
                joint("left", Some(HIPS), Vec3::new(-0.1, -1.0, 0.0)),
                joint("right", Some(HIPS), Vec3::new(0.1, -1.0, 0.0)),
            ],
            root: Transform::IDENTITY,
        }
    }

    fn landmarks() -> Landmarks {
        Landmarks { hips: HIPS, left_foot: LEFT, right_foot: RIGHT }
    }

    /// A clip that moves `joint`'s translation linearly from `from` to `to` over
    /// `seconds`.
    fn slide(clip: &mut AnimationClip, key: &str, from: Vec3, to: Vec3, seconds: f32) {
        let curve = UnevenSampleAutoCurve::new([(0.0, from), (seconds, to)]).expect("two samples");
        clip.add_curve_to_target(skeleton_target(key), AnimatableCurve::new(animated_field!(Transform::translation), curve));
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1.0e-4
    }

    #[test]
    fn a_skeleton_at_rest_places_its_joints_by_the_forward_pass() {
        let shape = shape();
        let locals: Vec<Transform> = shape.joints.iter().map(|j| j.rest).collect();
        let mut worlds = Vec::new();
        forward_pass(&shape, &locals, &mut worlds);
        assert!(close(position(&worlds, HIPS), Vec3::new(0.0, 1.0, 0.0)));
        assert!(close(position(&worlds, LEFT), Vec3::new(-0.1, 0.0, 0.0)));
        assert!(close(position(&worlds, RIGHT), Vec3::new(0.1, 0.0, 0.0)));
    }

    #[test]
    fn the_root_transform_scales_and_turns_the_whole_skeleton() {
        // An armature in centimetres: scale 0.01 over joints 100 times as far out.
        let mut shape = SkeletonShape {
            joints: vec![
                joint("hips", None, Vec3::new(0.0, 100.0, 0.0)),
                joint("left", Some(HIPS), Vec3::new(-10.0, -100.0, 0.0)),
                joint("right", Some(HIPS), Vec3::new(10.0, -100.0, 0.0)),
            ],
            root: Transform::from_scale(Vec3::splat(0.01)),
        };
        let locals: Vec<Transform> = shape.joints.iter().map(|j| j.rest).collect();
        let mut worlds = Vec::new();
        forward_pass(&shape, &locals, &mut worlds);
        assert!(close(position(&worlds, HIPS), Vec3::new(0.0, 1.0, 0.0)), "metres: {:?}", position(&worlds, HIPS));
        assert!(close(position(&worlds, LEFT), Vec3::new(-0.1, 0.0, 0.0)));

        // Turned a quarter about the vertical: the left foot (-x) goes to +z.
        shape.root.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        forward_pass(&shape, &locals, &mut worlds);
        assert!(close(position(&worlds, LEFT), Vec3::new(0.0, 0.0, 0.1)), "{:?}", position(&worlds, LEFT));
    }

    #[test]
    fn a_curve_sets_what_it_animates_and_a_joint_it_leaves_out_keeps_its_rest() {
        let shape = shape();
        let mut clip = AnimationClip::default();
        slide(&mut clip, "left", Vec3::new(-0.1, -1.0, 0.0), Vec3::new(-0.1, -1.0, 1.0), 1.0);
        let mut locals = Vec::new();
        sample_locals(&clip, &shape, 0.5, &mut locals);
        assert!(close(locals[LEFT].translation, Vec3::new(-0.1, -1.0, 0.5)), "halfway along: {:?}", locals[LEFT].translation);
        assert_eq!(locals[RIGHT], shape.joints[RIGHT].rest, "no curve: the rest transform");
        assert_eq!(locals[HIPS], shape.joints[HIPS].rest);
        sample_locals(&clip, &shape, 9.0, &mut locals);
        assert!(close(locals[LEFT].translation, Vec3::new(-0.1, -1.0, 1.0)), "past the end holds the end");
    }

    #[test]
    fn frames_are_sampled_at_the_frame_rate() {
        let shape = shape();
        let mut clip = AnimationClip::default();
        slide(&mut clip, "left", Vec3::new(-0.1, -1.0, 0.0), Vec3::new(-0.1, -1.0, 1.0), 1.0);
        // Not looping, 10 fps over a second: 11 frames, the last at the end.
        let frames = sample_frames(&clip, &shape, landmarks(), 10.0, false, HipsMode::PinnedToRest).unwrap();
        assert_eq!(frames.len(), 11);
        assert!((frames[5].left_foot.z - 0.5).abs() < 1.0e-4);
        assert!((frames[10].left_foot.z - 1.0).abs() < 1.0e-4);
        // Looping: 10 frames, the end is not repeated.
        let frames = sample_frames(&clip, &shape, landmarks(), 10.0, true, HipsMode::PinnedToRest).unwrap();
        assert_eq!(frames.len(), 10);
        assert!((frames[9].left_foot.z - 0.9).abs() < 1.0e-4);
    }

    #[test]
    fn the_hips_are_pinned_to_their_rest_unless_the_clip_is_to_keep_them() {
        let shape = shape();
        let mut clip = AnimationClip::default();
        slide(&mut clip, "hips", Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 2.0, 0.0), 1.0);
        let pinned = sample_frames(&clip, &shape, landmarks(), 10.0, false, HipsMode::PinnedToRest).unwrap();
        assert!(pinned.iter().all(|f| (f.hips.y - 1.0).abs() < 1.0e-4), "pinned hips stay up 1 m");
        assert!(pinned.iter().all(|f| (f.left_foot.y - 0.0).abs() < 1.0e-4), "and the feet with them");
        let authored = sample_frames(&clip, &shape, landmarks(), 10.0, false, HipsMode::Authored).unwrap();
        assert!((authored[10].hips.y - 2.0).abs() < 1.0e-4, "authored hips rise");
        assert!((authored[10].left_foot.y - 1.0).abs() < 1.0e-4, "the feet follow them up");
    }

    #[test]
    fn frames_are_measured_from_the_hips_and_the_soles_at_rest() {
        let shape = shape();
        let reference = reference_from_rest(&shape, landmarks(), Quat::IDENTITY).unwrap();
        assert!(close(reference.hips, Vec3::new(0.0, 1.0, 0.0)));
        assert_eq!(reference.ground, 0.0);
        let raw = [RawFrame {
            hips: Vec3::new(0.0, 1.0, 0.0),
            left_foot: Vec3::new(-0.1, 0.2, 0.3),
            right_foot: Vec3::new(0.1, 0.0, -0.3),
        }];
        let frames = pose_frames(&raw, &reference);
        assert!(close(frames[0].left_foot, Vec3::new(-0.1, 0.2, 0.3)));
        assert!(close(frames[0].right_foot, Vec3::new(0.1, 0.0, -0.3)));
        assert!((frames[0].hips_height - 1.0).abs() < 1.0e-5);

        // A skeleton that faces +z in its own frame is turned half a circle so
        // that forward is -z: a foot in front (+z) is then at -z.
        let turned = Reference { frame: Quat::from_rotation_y(std::f32::consts::PI), ..reference };
        let frames = pose_frames(&raw, &turned);
        assert!(close(frames[0].left_foot, Vec3::new(0.1, 0.2, -0.3)), "{:?}", frames[0].left_foot);
    }

    #[test]
    fn a_clip_becomes_database_frames_with_its_root_motion() {
        let shape = shape();
        let mut clip = AnimationClip::default();
        slide(&mut clip, "left", Vec3::new(-0.1, -1.0, -0.3), Vec3::new(-0.1, -1.0, 0.3), 1.0);
        let reference = reference_from_rest(&shape, landmarks(), Quat::IDENTITY).unwrap();
        let spec = ClipSpec { name: "walk".into(), tags: vec!["locomotion".into()], fps: 10.0, speed: 1.45, ..Default::default() };
        let clip_frames = clip_frames(&clip, &shape, landmarks(), &reference, &spec).unwrap();
        assert_eq!(clip_frames.name, "walk");
        assert_eq!(clip_frames.frames.len(), 10, "a looping clip: 10 frames at 10 fps");
        assert_eq!(clip_frames.root.len(), 10);
        assert!((clip_frames.root[0].velocity.y + 1.45).abs() < 1.0e-5, "forward is -z");
        assert!((clip_frames.duration() - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn input_that_cannot_be_sampled_is_refused() {
        let mut clip = AnimationClip::default();
        slide(&mut clip, "left", Vec3::ZERO, Vec3::X, 1.0);
        let shape = shape();
        let sample = |shape: &SkeletonShape, lm: Landmarks, fps: f32, clip: &AnimationClip| {
            sample_frames(clip, shape, lm, fps, false, HipsMode::PinnedToRest)
        };
        assert!(sample(&shape, landmarks(), 0.0, &clip).is_err(), "no frame rate");
        assert!(sample(&shape, Landmarks { hips: 9, ..landmarks() }, 10.0, &clip).is_err(), "a landmark out of range");
        assert!(sample(&shape, landmarks(), 10.0, &AnimationClip::default()).is_err(), "an empty clip");
        let mut backwards = shape.clone();
        backwards.joints[HIPS].parent = Some(LEFT);
        assert!(backwards.validate().is_err(), "a parent after its child");
        assert!(SkeletonShape { joints: Vec::new(), root: Transform::IDENTITY }.validate().is_err());
        assert!(reference_from_rest(&backwards, landmarks(), Quat::IDENTITY).is_err());
    }
}

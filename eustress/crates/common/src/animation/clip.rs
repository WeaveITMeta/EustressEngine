//! # Clips
//!
//! A `KeyframeSequence` record is TOML, one file per clip: `_instance.toml` in
//! the sequence's folder, or a `.anim.toml` file under `assets/animations/`.
//! This module reads one into a [`Sequence`], and builds a Bevy
//! `AnimationClip` from it for a given rig: one rotation curve and one
//! translation curve per posed joint, each evaluating Roblox's pose easing
//! exactly per segment, with nothing baked.
//!
//! ```toml
//! [metadata]
//! class_name = "KeyframeSequence"
//! name = "Wave"
//!
//! [keyframe_sequence]
//! loop = false
//! priority = "Action"
//!
//! [[keyframes]]
//! time = 0.3
//! name = "Raised"
//!
//! [keyframes.poses.RightUpperArm]
//! parent = "UpperTorso"
//! rotation = [0.0, 0.0, 150.0]   # degrees, Orientation order; or a quaternion x, y, z, w
//! easing = "CubicV2"
//! direction = "Out"
//!
//! [[keyframes.markers]]
//! name = "Wave"
//! value = "start"
//! ```
//!
//! Positions and `authored_hip_height` are metres unless `[metadata] unit`
//! names another unit.
//! Rotations are a quaternion (4 numbers), three angles in degrees applied in
//! the order Roblox's `Orientation` uses (3 numbers), or a `cframe` of 12
//! numbers in Roblox's order (position, then the rotation matrix by rows).
//!
//! **Structural poses.** A pose with `weight = 0` keys nothing: Roblox's
//! Animation Editor saves every unkeyed parent that way, only to hold the
//! pose hierarchy together, so an arm-only clip leaves the torso to the
//! tracks below it.
//!
//! **Rest-relative poses.** On parts, a pose is the `Motor6D`'s `Transform`,
//! whose identity is the rig's authored pose. On a skeleton, a pose is
//! relative to the bone's bind pose, as a Roblox `Bone`'s `Transform` is.

use bevy::animation::animated_field;
use bevy::animation::animation_curves::{AnimatableCurve, AnimatedField, AnimationCurve};
use bevy::animation::{AnimationClip, AnimationTargetId};
use bevy::math::curve::{ConstantCurve, Curve, Interval};
use bevy::prelude::*;

use super::easing::{ease, PoseEasingDirection, PoseEasingStyle};
use crate::datamodel::animation::{ClipInfo, Priority};

/// The largest record the loader reads.
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
/// The most keyframes one record may hold.
pub const MAX_KEYFRAMES: usize = 10_000;
/// The most poses one keyframe may hold.
pub const MAX_POSES_PER_KEYFRAME: usize = 512;
/// Roblox's default `AuthoredHipHeight`, 2 studs, in metres at the
/// importer's 0.3048 m per stud.
pub const DEFAULT_AUTHORED_HIP_HEIGHT: f32 = 0.6096;
/// A clip is never shorter than this, so its curves have a domain.
const MIN_LENGTH: f32 = 1.0e-3;

// ============================================================================
// The pose a joint takes
// ============================================================================

/// The animated value of a `Motor6D` rig's joint: its `Transform` (rotation
/// and translation; a joint never scales). Lives on a joint entity the
/// Animator runtime owns, and the forward pass applies it to `Part1`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct JointPose {
    pub rotation: Quat,
    pub translation: Vec3,
}

impl Default for JointPose {
    fn default() -> Self {
        Self { rotation: Quat::IDENTITY, translation: Vec3::ZERO }
    }
}

// ============================================================================
// The record
// ============================================================================

/// One pose in one keyframe.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseData {
    /// The joint: a bone's name, or a `Motor6D`'s `Part1` name.
    pub joint: String,
    /// The pose above it: a `Motor6D`'s `Part0` name. Needed only where two
    /// joints share a name.
    pub parent: Option<String>,
    pub position: Vec3,
    pub rotation: Quat,
    pub style: PoseEasingStyle,
    pub direction: PoseEasingDirection,
    /// 0 marks a structural pose, which keys nothing; any other value keys
    /// the joint fully, as in Roblox.
    pub weight: f32,
}

/// One `NumberPose`: a number channel such as a face control.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberData {
    pub name: String,
    pub value: f32,
    pub style: PoseEasingStyle,
    pub direction: PoseEasingDirection,
}

/// One keyframe.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeData {
    pub time: f32,
    pub name: String,
    pub poses: Vec<PoseData>,
    pub markers: Vec<(String, String)>,
    pub numbers: Vec<NumberData>,
}

/// A parsed `KeyframeSequence`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub looped: bool,
    pub priority: Priority,
    /// Roblox's `AuthoredHipHeight`, in metres like every length here: the
    /// file's `[metadata] unit` applies to it as it does to positions.
    pub authored_hip_height: f32,
    /// The rig the clip was authored for (`R15`, `R6`, `Humanoid`), checked
    /// when it binds.
    pub rig: Option<String>,
    /// Keyframes in time order.
    pub keyframes: Vec<KeyframeData>,
}

impl Sequence {
    /// The last keyframe's time: the clip's length.
    pub fn length(&self) -> f32 {
        self.keyframes.iter().map(|k| k.time).fold(0.0_f32, f32::max)
    }

    /// What the track model needs to know about this clip.
    pub fn info(&self) -> ClipInfo {
        let mut markers = Vec::new();
        for k in &self.keyframes {
            for (name, value) in &k.markers {
                markers.push((k.time, name.clone(), value.clone()));
            }
        }
        ClipInfo {
            length: self.length(),
            looped: self.looped,
            priority: self.priority,
            keyframes: self.keyframes.iter().map(|k| (k.time, k.name.clone())).collect(),
            markers,
        }
    }

    /// The poses of each joint over time, in keyframe order. Structural
    /// poses are left out.
    pub fn channels(&self) -> Vec<JointChannel> {
        let mut out: Vec<JointChannel> = Vec::new();
        for k in &self.keyframes {
            for p in k.poses.iter().filter(|p| p.weight > 0.0) {
                let key = ChannelKey {
                    time: k.time,
                    position: p.position,
                    rotation: p.rotation,
                    style: p.style,
                    direction: p.direction,
                };
                match out.iter_mut().find(|c| c.joint == p.joint && c.parent == p.parent) {
                    Some(c) => c.keys.push(key),
                    None => out.push(JointChannel { joint: p.joint.clone(), parent: p.parent.clone(), keys: vec![key] }),
                }
            }
        }
        out
    }
}

/// One joint's poses over time.
#[derive(Debug, Clone, PartialEq)]
pub struct JointChannel {
    pub joint: String,
    pub parent: Option<String>,
    pub keys: Vec<ChannelKey>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelKey {
    pub time: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub style: PoseEasingStyle,
    pub direction: PoseEasingDirection,
}

/// The name a record without `[metadata] name` gets: its folder's name for
/// an `_instance.toml`, else the file's name without `.anim.toml`.
pub fn record_fallback_name(file: &std::path::Path) -> String {
    let file_name = file.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    let name = if file_name.eq_ignore_ascii_case("_instance.toml") {
        file.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or_default()
    } else {
        file_name.trim_end_matches(".toml").trim_end_matches(".anim")
    };
    if name.is_empty() { "KeyframeSequence".to_string() } else { name.to_string() }
}

/// Read a `KeyframeSequence` record. Returns the sequence and the problems
/// worth reporting (unknown easing names, poses skipped); an unreadable or
/// oversized record is an error.
pub fn parse_sequence(text: &str, fallback_name: &str) -> Result<(Sequence, Vec<String>), String> {
    if text.len() > MAX_RECORD_BYTES {
        return Err(format!("clip record is {} bytes; the limit is {MAX_RECORD_BYTES}", text.len()));
    }
    let doc: toml::Value = toml::from_str(text).map_err(|e| format!("clip record: {e}"))?;
    let mut problems = Vec::new();

    let metadata = doc.get("metadata");
    let name = metadata
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    let unit_scale = match metadata.and_then(|m| m.get("unit")).and_then(|u| u.as_str()) {
        None => 1.0,
        Some(symbol) => match crate::units::Unit::from_symbol(symbol) {
            Some(unit) => unit.to_meters() as f32,
            None => {
                problems.push(format!("unknown unit {symbol:?}; positions read as metres"));
                1.0
            }
        },
    };

    let props = doc.get("keyframe_sequence");
    let looped = props.and_then(|p| p.get("loop")).and_then(|v| v.as_bool()).unwrap_or(true);
    let priority = match props.and_then(|p| p.get("priority")) {
        None => Priority::Action,
        Some(toml::Value::String(s)) => Priority::from_name(s).unwrap_or_else(|| {
            problems.push(format!("unknown priority {s:?}; using Action"));
            Priority::Action
        }),
        Some(toml::Value::Integer(n)) => {
            Priority::from_roblox_value((*n).clamp(0, u32::MAX as i64) as u32).unwrap_or(Priority::Action)
        }
        Some(other) => {
            problems.push(format!("priority should be a name, got {}", other.type_str()));
            Priority::Action
        }
    };
    let authored_hip_height = props
        .and_then(|p| p.get("authored_hip_height"))
        .and_then(number)
        .filter(|h| h.is_finite() && *h > 0.0)
        .map(|h| h * unit_scale)
        .unwrap_or(DEFAULT_AUTHORED_HIP_HEIGHT);
    let rig = props.and_then(|p| p.get("rig")).and_then(|v| v.as_str()).map(str::to_string);

    let mut keyframes = Vec::new();
    if let Some(list) = doc.get("keyframes") {
        let list = list.as_array().ok_or("keyframes must be an array of tables ([[keyframes]])")?;
        if list.len() > MAX_KEYFRAMES {
            return Err(format!("{} keyframes; the limit is {MAX_KEYFRAMES}", list.len()));
        }
        for (i, k) in list.iter().enumerate() {
            let time = k
                .get("time")
                .and_then(number)
                .ok_or_else(|| format!("keyframe {i} has no time"))?;
            if !time.is_finite() || time < 0.0 {
                return Err(format!("keyframe {i} has an invalid time {time}"));
            }
            let kname = k.get("name").and_then(|v| v.as_str()).unwrap_or("Keyframe").to_string();

            let mut poses = Vec::new();
            if let Some(table) = k.get("poses").and_then(|p| p.as_table()) {
                if table.len() > MAX_POSES_PER_KEYFRAME {
                    return Err(format!("keyframe {i} has {} poses; the limit is {MAX_POSES_PER_KEYFRAME}", table.len()));
                }
                for (joint, pose) in table {
                    match parse_pose(joint, pose, unit_scale, &mut problems) {
                        Ok(p) => poses.push(p),
                        Err(e) => problems.push(format!("keyframe {i}, pose {joint}: {e}; skipped")),
                    }
                }
            }

            let mut markers = Vec::new();
            if let Some(list) = k.get("markers").and_then(|m| m.as_array()) {
                for m in list {
                    let Some(mname) = m.get("name").and_then(|v| v.as_str()) else {
                        problems.push(format!("keyframe {i} has a marker with no name; skipped"));
                        continue;
                    };
                    let value = m.get("value").and_then(|v| v.as_str()).unwrap_or_default();
                    markers.push((mname.to_string(), value.to_string()));
                }
            }

            let mut numbers = Vec::new();
            if let Some(table) = k.get("numbers").and_then(|n| n.as_table()) {
                for (nname, v) in table {
                    let (value, style, direction) = match v {
                        toml::Value::Table(t) => (
                            t.get("value").and_then(number),
                            easing_style(t.get("easing"), &mut problems),
                            easing_direction(t.get("direction"), &mut problems),
                        ),
                        other => (number(other), PoseEasingStyle::Linear, PoseEasingDirection::In),
                    };
                    match value.filter(|v| v.is_finite()) {
                        Some(value) => numbers.push(NumberData { name: nname.clone(), value, style, direction }),
                        None => problems.push(format!("keyframe {i}, number {nname}: not a finite number; skipped")),
                    }
                }
            }

            keyframes.push(KeyframeData { time, name: kname, poses, markers, numbers });
        }
    }
    keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));

    Ok((Sequence { name, looped, priority, authored_hip_height, rig, keyframes }, problems))
}

fn number(v: &toml::Value) -> Option<f32> {
    match v {
        toml::Value::Float(f) => Some(*f as f32),
        toml::Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

fn numbers(v: &toml::Value) -> Option<Vec<f32>> {
    v.as_array()?.iter().map(number).collect()
}

fn easing_style(v: Option<&toml::Value>, problems: &mut Vec<String>) -> PoseEasingStyle {
    match v {
        None => PoseEasingStyle::Linear,
        Some(toml::Value::String(s)) => PoseEasingStyle::from_name(s).unwrap_or_else(|| {
            problems.push(format!("unknown easing {s:?}; using Linear"));
            PoseEasingStyle::Linear
        }),
        Some(toml::Value::Integer(n)) => {
            PoseEasingStyle::from_roblox_value((*n).clamp(0, 255) as u32).unwrap_or(PoseEasingStyle::Linear)
        }
        Some(_) => PoseEasingStyle::Linear,
    }
}

fn easing_direction(v: Option<&toml::Value>, problems: &mut Vec<String>) -> PoseEasingDirection {
    match v {
        None => PoseEasingDirection::In,
        Some(toml::Value::String(s)) => PoseEasingDirection::from_name(s).unwrap_or_else(|| {
            problems.push(format!("unknown easing direction {s:?}; using In"));
            PoseEasingDirection::In
        }),
        Some(toml::Value::Integer(n)) => {
            PoseEasingDirection::from_roblox_value((*n).clamp(0, 255) as u32).unwrap_or(PoseEasingDirection::In)
        }
        Some(_) => PoseEasingDirection::In,
    }
}

fn parse_pose(
    joint: &str,
    pose: &toml::Value,
    unit_scale: f32,
    problems: &mut Vec<String>,
) -> Result<PoseData, String> {
    let table = pose.as_table().ok_or("a pose must be a table")?;
    let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());

    let (mut position, mut rotation) = (Vec3::ZERO, Quat::IDENTITY);
    if let Some(cf) = table.get("cframe") {
        let v = numbers(cf).filter(|v| v.len() == 12 && finite(v)).ok_or("cframe needs 12 finite numbers")?;
        position = Vec3::new(v[0], v[1], v[2]);
        // Roblox's matrix, by rows: the columns are the right, up and back vectors.
        let m = Mat3::from_cols(Vec3::new(v[3], v[6], v[9]), Vec3::new(v[4], v[7], v[10]), Vec3::new(v[5], v[8], v[11]));
        rotation = Quat::from_mat3(&m).normalize();
    }
    if let Some(p) = table.get("position") {
        let v = numbers(p).filter(|v| v.len() == 3 && finite(v)).ok_or("position needs 3 finite numbers")?;
        position = Vec3::new(v[0], v[1], v[2]);
    }
    if let Some(r) = table.get("rotation") {
        let v = numbers(r).filter(|v| finite(v)).ok_or("rotation needs finite numbers")?;
        rotation = match v.len() {
            4 => {
                let q = Quat::from_xyzw(v[0], v[1], v[2], v[3]);
                if q.length_squared() < 1e-12 {
                    return Err("a zero quaternion is not a rotation".into());
                }
                q.normalize()
            }
            3 => Quat::from_euler(EulerRot::YXZ, v[1].to_radians(), v[0].to_radians(), v[2].to_radians()),
            n => return Err(format!("rotation needs 3 angles or 4 quaternion numbers, got {n}")),
        };
    }
    let parent = table.get("parent").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
    let weight = table.get("weight").and_then(number).filter(|w| w.is_finite()).unwrap_or(1.0);

    Ok(PoseData {
        joint: joint.to_string(),
        parent,
        position: position * unit_scale,
        rotation,
        style: easing_style(table.get("easing"), problems),
        direction: easing_direction(table.get("direction"), problems),
        weight,
    })
}

// ============================================================================
// Curves
// ============================================================================

#[derive(Debug, Clone, Copy, Reflect)]
struct RotationKey {
    time: f32,
    value: Quat,
    style: PoseEasingStyle,
    direction: PoseEasingDirection,
}

#[derive(Debug, Clone, Copy, Reflect)]
struct TranslationKey {
    time: f32,
    value: Vec3,
    style: PoseEasingStyle,
    direction: PoseEasingDirection,
}

/// A joint's rotation over a clip, eased per segment as Roblox eases poses.
/// Holds its first key before it and its last key after it.
#[derive(Debug, Clone, Reflect)]
pub struct PoseRotationCurve {
    keys: Vec<RotationKey>,
    end: f32,
}

/// A joint's translation over a clip, eased per segment.
#[derive(Debug, Clone, Reflect)]
pub struct PoseTranslationCurve {
    keys: Vec<TranslationKey>,
    end: f32,
}

fn clip_domain(end: f32) -> Interval {
    Interval::new(0.0, end.max(MIN_LENGTH)).unwrap_or(Interval::UNIT)
}

/// The segment `t` falls in: its start key's index and the eased progress.
fn segment(times: impl Fn(usize) -> f32, count: usize, t: f32, eased: impl Fn(usize, f32) -> f32) -> (usize, usize, f32) {
    if count <= 1 || t <= times(0) {
        return (0, 0, 0.0);
    }
    let last = count - 1;
    if t >= times(last) {
        return (last, last, 0.0);
    }
    // First key after `t`.
    let (mut lo, mut hi) = (0usize, last);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if times(mid) <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let span = times(hi) - times(lo);
    let progress = if span > 0.0 { ((t - times(lo)) / span).clamp(0.0, 1.0) } else { 1.0 };
    (lo, hi, eased(lo, progress))
}

impl Curve<Quat> for PoseRotationCurve {
    fn domain(&self) -> Interval {
        clip_domain(self.end)
    }

    fn sample_unchecked(&self, t: f32) -> Quat {
        if self.keys.is_empty() {
            return Quat::IDENTITY;
        }
        let (a, b, e) = segment(
            |i| self.keys[i].time,
            self.keys.len(),
            t,
            |i, p| ease(self.keys[i].style, self.keys[i].direction, p),
        );
        if a == b {
            return self.keys[a].value;
        }
        self.keys[a].value.slerp(self.keys[b].value, e)
    }
}

impl Curve<Vec3> for PoseTranslationCurve {
    fn domain(&self) -> Interval {
        clip_domain(self.end)
    }

    fn sample_unchecked(&self, t: f32) -> Vec3 {
        if self.keys.is_empty() {
            return Vec3::ZERO;
        }
        let (a, b, e) = segment(
            |i| self.keys[i].time,
            self.keys.len(),
            t,
            |i, p| ease(self.keys[i].style, self.keys[i].direction, p),
        );
        if a == b {
            return self.keys[a].value;
        }
        self.keys[a].value.lerp(self.keys[b].value, e)
    }
}

// ============================================================================
// Rigs
// ============================================================================

/// What a rig's joints are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RigKind {
    /// Bones of a skinned body, animated through their `Transform`.
    Skeleton,
    /// Parts joined by `Motor6D`s, animated through [`JointPose`].
    Parts,
}

/// One animatable joint of a bound rig.
#[derive(Debug, Clone)]
pub struct RigJoint {
    /// Skeleton: the canonical bone key. Parts: `Part1`'s name.
    pub key: String,
    /// Parts: `Part0`'s name.
    pub parent_key: Option<String>,
    pub target: AnimationTargetId,
    pub entity: Entity,
    /// Skeleton: the bone's bind-pose local transform. Parts: identity.
    pub rest: Transform,
}

/// A bound rig's joints, in a fixed order that coverage bit sets index.
#[derive(Debug, Clone)]
pub struct RigJoints {
    pub kind: RigKind,
    pub joints: Vec<RigJoint>,
    /// The skeleton root's uniform scale (0.01 for Mixamo exports): metres
    /// divide by it to become bone-local units.
    pub armature_scale: f32,
    /// `*.rig.toml` `[bones]`: a node name in a file, and the standard bone.
    pub aliases: Vec<(String, String)>,
}

impl RigJoints {
    /// The joint a pose names, if this rig has it.
    pub fn find(&self, joint: &str, parent: Option<&str>) -> Option<usize> {
        match self.kind {
            RigKind::Skeleton => {
                let raw = self
                    .aliases
                    .iter()
                    .find(|(from, _)| from == joint)
                    .map(|(_, to)| to.as_str())
                    .unwrap_or(joint);
                let key = crate::avatar::rig::canonical_bone_key(raw);
                if let Some(i) = self.joints.iter().position(|j| j.key == key) {
                    return Some(i);
                }
                // `Pelvis`, `Chest` and the other spellings of a standard bone.
                let bone = crate::avatar::rig::HumanoidBone::from_canonical(&key)?;
                bone.aliases().iter().find_map(|alias| self.joints.iter().position(|j| j.key == *alias))
            }
            RigKind::Parts => self.joints.iter().position(|j| {
                j.key == joint && parent.map_or(true, |p| j.parent_key.as_deref() == Some(p))
            }),
        }
    }
}

/// A clip built for one rig: the clip, and the joints it animates.
#[derive(Debug)]
pub struct BuiltClip {
    pub clip: AnimationClip,
    /// Indices into the rig's joints, ascending.
    pub coverage: Vec<usize>,
    /// Joints the clip poses that this rig does not have.
    pub unmatched: Vec<String>,
}

/// Build `sequence` for `rig`.
pub fn build_sequence_clip(sequence: &Sequence, rig: &RigJoints) -> BuiltClip {
    let end = sequence.length().max(MIN_LENGTH);
    let mut clip = AnimationClip::default();
    let mut coverage = Vec::new();
    let mut unmatched = Vec::new();

    for channel in sequence.channels() {
        let Some(index) = rig.find(&channel.joint, channel.parent.as_deref()) else {
            unmatched.push(channel.joint.clone());
            continue;
        };
        if coverage.contains(&index) {
            // Two channels land on one joint (a name and its alias): the first wins.
            continue;
        }
        let joint = &rig.joints[index];
        let scale = rig.armature_scale.abs().max(1.0e-6);
        let (rot_keys, pos_keys): (Vec<RotationKey>, Vec<TranslationKey>) = channel
            .keys
            .iter()
            .map(|k| {
                let (rotation, translation) = match rig.kind {
                    // Relative to the bind pose, as a Roblox Bone's Transform is;
                    // metres become bone-local units under the armature's scale.
                    RigKind::Skeleton => (
                        joint.rest.rotation * k.rotation,
                        joint.rest.translation + joint.rest.rotation * (k.position / scale),
                    ),
                    RigKind::Parts => (k.rotation, k.position),
                };
                (
                    RotationKey { time: k.time, value: rotation, style: k.style, direction: k.direction },
                    TranslationKey { time: k.time, value: translation, style: k.style, direction: k.direction },
                )
            })
            .unzip();
        let rotation = PoseRotationCurve { keys: rot_keys, end };
        let translation = PoseTranslationCurve { keys: pos_keys, end };
        match rig.kind {
            RigKind::Skeleton => {
                clip.add_curve_to_target(joint.target, AnimatableCurve::new(animated_field!(Transform::rotation), rotation));
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(animated_field!(Transform::translation), translation),
                );
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(Transform::scale),
                        ConstantCurve::new(Interval::EVERYWHERE, joint.rest.scale),
                    ),
                );
            }
            RigKind::Parts => {
                clip.add_curve_to_target(joint.target, AnimatableCurve::new(animated_field!(JointPose::rotation), rotation));
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(animated_field!(JointPose::translation), translation),
                );
            }
        }
        coverage.push(index);
    }
    clip.set_duration(end);
    coverage.sort_unstable();
    BuiltClip { clip, coverage, unmatched }
}

/// A clip holding every joint at rest. Every class of the rig's graph plays
/// it, so a joint no track animates returns to rest and a partial weight
/// blends toward rest.
pub fn rest_clip(rig: &RigJoints) -> AnimationClip {
    let mut clip = AnimationClip::default();
    for joint in &rig.joints {
        match rig.kind {
            RigKind::Skeleton => {
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(Transform::rotation),
                        ConstantCurve::new(Interval::EVERYWHERE, joint.rest.rotation),
                    ),
                );
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(Transform::translation),
                        ConstantCurve::new(Interval::EVERYWHERE, joint.rest.translation),
                    ),
                );
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(Transform::scale),
                        ConstantCurve::new(Interval::EVERYWHERE, joint.rest.scale),
                    ),
                );
            }
            RigKind::Parts => {
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(JointPose::rotation),
                        ConstantCurve::new(Interval::EVERYWHERE, Quat::IDENTITY),
                    ),
                );
                clip.add_curve_to_target(
                    joint.target,
                    AnimatableCurve::new(
                        animated_field!(JointPose::translation),
                        ConstantCurve::new(Interval::EVERYWHERE, Vec3::ZERO),
                    ),
                );
            }
        }
    }
    clip
}

/// Make a glTF clip complete for a skeleton: every joint it animates gets a
/// rotation, a translation and a scale curve, the missing ones held at the
/// joint's rest value, so no property of an animated joint is left without a
/// contributor in the blend. Returns the joints the clip animates.
pub fn complete_skeleton_clip(clip: &mut AnimationClip, rig: &RigJoints) -> Vec<usize> {
    let rotation_ref = AnimatableCurve::new(
        animated_field!(Transform::rotation),
        ConstantCurve::new(Interval::EVERYWHERE, Quat::IDENTITY),
    );
    let translation_ref = AnimatableCurve::new(
        animated_field!(Transform::translation),
        ConstantCurve::new(Interval::EVERYWHERE, Vec3::ZERO),
    );
    let scale_ref =
        AnimatableCurve::new(animated_field!(Transform::scale), ConstantCurve::new(Interval::EVERYWHERE, Vec3::ONE));

    let mut coverage = Vec::new();
    let mut missing: Vec<(usize, bool, bool, bool)> = Vec::new();
    for (i, joint) in rig.joints.iter().enumerate() {
        let Some(curves) = clip.curves_for_target(joint.target) else { continue };
        coverage.push(i);
        let has = |reference: &dyn AnimationCurve| curves.iter().any(|c| c.0.evaluator_id() == reference.evaluator_id());
        let (r, t, s) = (has(&rotation_ref), has(&translation_ref), has(&scale_ref));
        if !(r && t && s) {
            missing.push((i, r, t, s));
        }
    }
    for (i, r, t, s) in missing {
        let joint = &rig.joints[i];
        if !r {
            clip.add_curve_to_target(
                joint.target,
                AnimatableCurve::new(
                    animated_field!(Transform::rotation),
                    ConstantCurve::new(Interval::EVERYWHERE, joint.rest.rotation),
                ),
            );
        }
        if !t {
            clip.add_curve_to_target(
                joint.target,
                AnimatableCurve::new(
                    animated_field!(Transform::translation),
                    ConstantCurve::new(Interval::EVERYWHERE, joint.rest.translation),
                ),
            );
        }
        if !s {
            clip.add_curve_to_target(
                joint.target,
                AnimatableCurve::new(
                    animated_field!(Transform::scale),
                    ConstantCurve::new(Interval::EVERYWHERE, joint.rest.scale),
                ),
            );
        }
    }
    coverage
}

/// The target id of a skeleton bone, by its canonical key.
pub fn skeleton_target(key: &str) -> AnimationTargetId {
    AnimationTargetId::from_iter(["eustress_rig", key])
}

/// The target id of a `Motor6D` joint, by its parts' names.
pub fn joint_target(part0: &str, part1: &str) -> AnimationTargetId {
    AnimationTargetId::from_iter(["eustress_joint", part0, part1])
}

/// The angle between two rotations, in radians, precise near zero. `Quat::angle_between`
/// goes through `acos` of the dot product, which turns one float of rounding in two
/// equal rotations into about 7e-4 rad; this goes through `atan2`.
#[cfg(test)]
pub(crate) fn rotation_gap(a: Quat, b: Quat) -> f32 {
    let d = a * b.inverse();
    2.0 * d.xyz().length().atan2(d.w.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAVE: &str = r#"
[metadata]
class_name = "KeyframeSequence"
name = "Wave"

[keyframe_sequence]
loop = false
priority = "Action"

[[keyframes]]
time = 0.0

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]

[[keyframes]]
time = 0.4
name = "Raised"

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
rotation = [0.0, 0.0, 90.0]
easing = "CubicV2"
direction = "Out"

[[keyframes.markers]]
name = "Wave"
value = "start"
"#;

    #[test]
    fn a_record_parses_into_keyframes_poses_and_markers() {
        let (seq, problems) = parse_sequence(WAVE, "fallback").unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(seq.name, "Wave");
        assert!(!seq.looped);
        assert_eq!(seq.priority, Priority::Action);
        assert_eq!(seq.keyframes.len(), 2);
        assert_eq!(seq.length(), 0.4);
        let info = seq.info();
        assert_eq!(info.markers, vec![(0.4, "Wave".to_string(), "start".to_string())]);
        assert_eq!(info.keyframes[1].1, "Raised");
        let channels = seq.channels();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].parent.as_deref(), Some("UpperTorso"));
        let raised = channels[0].keys[1].rotation;
        let expected = Quat::from_rotation_z(90f32.to_radians());
        assert!(rotation_gap(raised, expected) < 1e-5, "{raised:?}");
    }

    #[test]
    fn defaults_follow_roblox() {
        let (seq, _) = parse_sequence("[[keyframes]]\ntime = 1.0\n", "Clip").unwrap();
        assert!(seq.looped, "Roblox's KeyframeSequence defaults Loop to true");
        assert_eq!(seq.priority, Priority::Action);
        assert_eq!(seq.name, "Clip");
        assert_eq!(seq.authored_hip_height, DEFAULT_AUTHORED_HIP_HEIGHT);
    }

    #[test]
    fn hip_height_is_a_length_in_the_files_unit() {
        let text = "[metadata]\nunit = \"ft\"\n[keyframe_sequence]\nauthored_hip_height = 2.0\n[[keyframes]]\ntime = 1.0\n";
        let (seq, _) = parse_sequence(text, "Clip").unwrap();
        assert!((seq.authored_hip_height - 0.6096).abs() < 1e-5, "{}", seq.authored_hip_height);
    }

    #[test]
    fn bad_records_are_refused_and_bad_poses_skipped() {
        assert!(parse_sequence("[[keyframes]]\nname = \"x\"\n", "c").is_err(), "a keyframe needs a time");
        assert!(parse_sequence("[[keyframes]]\ntime = -1.0\n", "c").is_err());
        let text = "[[keyframes]]\ntime = 0.0\n[keyframes.poses.Arm]\nrotation = [0.0, 0.0, 0.0, 0.0]\n";
        let (seq, problems) = parse_sequence(text, "c").unwrap();
        assert!(seq.keyframes[0].poses.is_empty());
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn a_unit_scales_positions_to_metres() {
        let text = "[metadata]\nunit = \"ft\"\n[[keyframes]]\ntime = 0.0\n[keyframes.poses.Root]\nposition = [0.0, 10.0, 0.0]\n";
        let (seq, _) = parse_sequence(text, "c").unwrap();
        let y = seq.keyframes[0].poses[0].position.y;
        assert!((y - 3.048).abs() < 1e-4, "{y}");
    }

    #[test]
    fn a_cframe_reads_roblox_matrix_order() {
        // 90 degrees about Y: right = (0, 0, -1), up = (0, 1, 0), back = (1, 0, 0).
        let text = "[[keyframes]]\ntime = 0.0\n[keyframes.poses.Head]\ncframe = [1, 2, 3, 0, 0, 1, 0, 1, 0, -1, 0, 0]\n";
        let (seq, problems) = parse_sequence(text, "c").unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let p = &seq.keyframes[0].poses[0];
        assert_eq!(p.position, Vec3::new(1.0, 2.0, 3.0));
        let expected = Quat::from_rotation_y(90f32.to_radians());
        assert!(rotation_gap(p.rotation, expected) < 1e-5, "{:?}", p.rotation);
    }

    #[test]
    fn the_curve_eases_between_keys_and_holds_outside_them() {
        let curve = PoseRotationCurve {
            keys: vec![
                RotationKey { time: 0.0, value: Quat::IDENTITY, style: PoseEasingStyle::Linear, direction: PoseEasingDirection::In },
                RotationKey {
                    time: 1.0,
                    value: Quat::from_rotation_z(1.0),
                    style: PoseEasingStyle::Linear,
                    direction: PoseEasingDirection::In,
                },
            ],
            end: 2.0,
        };
        let half = curve.sample_unchecked(0.5);
        assert!(rotation_gap(half, Quat::from_rotation_z(0.5)) < 1e-5);
        assert!(rotation_gap(curve.sample_unchecked(1.5), Quat::from_rotation_z(1.0)) < 1e-5);
        assert_eq!(curve.domain().end(), 2.0);
    }

    fn parts_rig() -> RigJoints {
        RigJoints {
            kind: RigKind::Parts,
            joints: vec![
                RigJoint {
                    key: "RightUpperArm".into(),
                    parent_key: Some("UpperTorso".into()),
                    target: joint_target("UpperTorso", "RightUpperArm"),
                    entity: Entity::PLACEHOLDER,
                    rest: Transform::IDENTITY,
                },
                RigJoint {
                    key: "LeftUpperArm".into(),
                    parent_key: Some("UpperTorso".into()),
                    target: joint_target("UpperTorso", "LeftUpperArm"),
                    entity: Entity::PLACEHOLDER,
                    rest: Transform::IDENTITY,
                },
            ],
            armature_scale: 1.0,
            aliases: Vec::new(),
        }
    }

    #[test]
    fn a_clip_covers_the_joints_it_poses_and_names_the_rest() {
        let (seq, _) = parse_sequence(WAVE, "w").unwrap();
        let built = build_sequence_clip(&seq, &parts_rig());
        assert_eq!(built.coverage, vec![0]);
        assert!(built.unmatched.is_empty());
        assert!(built.clip.curves_for_target(joint_target("UpperTorso", "RightUpperArm")).is_some());
        assert!(built.clip.curves_for_target(joint_target("UpperTorso", "LeftUpperArm")).is_none());
        assert!((built.clip.duration() - 0.4).abs() < 1e-6);
        let rest = rest_clip(&parts_rig());
        assert_eq!(rest.curves().len(), 2);
    }

    #[test]
    fn skeleton_joints_match_by_canonical_key_and_alias() {
        let rig = RigJoints {
            kind: RigKind::Skeleton,
            joints: vec![RigJoint {
                key: "hips".into(),
                parent_key: None,
                target: skeleton_target("hips"),
                entity: Entity::PLACEHOLDER,
                rest: Transform::IDENTITY,
            }],
            armature_scale: 0.01,
            aliases: vec![("Bip01 Pelvis".into(), "hips".into())],
        };
        assert_eq!(rig.find("mixamorig:Hips_001", None), Some(0));
        assert_eq!(rig.find("Pelvis", None), Some(0));
        assert_eq!(rig.find("Bip01 Pelvis", None), Some(0));
        assert_eq!(rig.find("LeftFoot", None), None);
    }
}

//! # The Animator runtime
//!
//! The Bevy side of every `Animator` in the tree:
//!
//! 1. **Bind.** An Animator animates its parent's Model. A Model whose root
//!    part is bound to an avatar is a skeleton rig (its bones, in
//!    `AvatarRig`); a Model with `Motor6D`s is a part rig (a joint entity per
//!    `Motor6D`, animated through [`JointPose`]).
//! 2. **Load.** Each track's `AnimationId` resolves to a clip
//!    ([`super::content`]): glTF clips load through the asset server and
//!    retarget onto the skeleton's canonical bones; `KeyframeSequence`s build
//!    from their record or from the live tree. The clip's length, keyframes
//!    and markers go back to the track model.
//! 3. **Drive.** Every frame the playing tracks become a graph of coverage
//!    classes ([`super::graph`]); each clip node's `ActiveAnimation` is
//!    paused, its time set from the track model and its weight from the
//!    blending rule. The track model is the one clock.
//!
//! A changed graph is threaded by Bevy a frame after its asset changes, so a
//! new graph goes in as a new asset and replaces the old one two frames
//! later; the old one keeps playing in between, and nothing glitches.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle};
use bevy::animation::{AnimatedBy, AnimationClip, AnimationPlayer, AnimationTargetId};
use bevy::prelude::*;
use tracing::{info, warn};

use super::clip::{
    build_sequence_clip, complete_skeleton_clip, joint_target, parse_sequence, record_fallback_name, rest_clip,
    skeleton_target, JointPose, RigJoint, RigJoints, RigKind, Sequence,
};
use super::content::{self, ClipSource, RigClips, Roots};
use super::graph::{build_graph, coverage_classes, ClassNodes, CoverageClass};
use super::tree::sequence_from_tree;
use super::{legacy_motion, AnimatorDriven, LiveTree, PoseReady};
use crate::avatar::rig::{AvatarRig, HumanoidBone};
use crate::avatar::AvatarDescriptor;
use crate::datamodel::animation::{effective_weights, AnimatorFrame, ClipInfo, Priority, TrackSample};
use crate::datamodel::{DataModel, DmValue, InstanceId, OutputLevel};

/// How often an unbound Animator retries its rig, in frames.
const BIND_RETRY_FRAMES: u32 = 10;
/// How often the Animator list is rebuilt while the tree keeps changing.
const SCAN_EVERY_FRAMES: u32 = 10;
/// A rig that plays nothing this long is posed anyway, so its feet calibrate.
const POSE_READY_FALLBACK_FRAMES: u32 = 300;
/// A new graph replaces the old one after Bevy has threaded it.
const GRAPH_SWAP_FRAMES: u32 = 2;

// ============================================================================
// Components and resources
// ============================================================================

/// Every Animator's runtime entity.
#[derive(Resource, Default)]
pub struct AnimatorIndex {
    tree: usize,
    structure: u64,
    frames_since_scan: u32,
    pub by_animator: HashMap<InstanceId, Entity>,
}

/// A skeleton's bind pose, captured the frame its rig binds, before any
/// animation or procedural layer has touched it.
#[derive(Component, Debug, Clone, Default)]
pub struct BindPose(pub HashMap<Entity, Transform>);

/// A part rig's joint: the `Motor6D` it stands for and the parts it joins.
#[derive(Component, Debug, Clone, Copy)]
pub struct Motor6DLink {
    pub part0: Entity,
    pub part1: Entity,
    pub c0: Transform,
    pub c1: Transform,
    pub enabled: bool,
}

/// The Bevy side of one `Animator`. Lives on an entity that also holds its
/// `AnimationPlayer` and `AnimationGraphHandle`.
#[derive(Component)]
pub struct AnimatorRuntime {
    pub animator: InstanceId,
    rig: Option<BoundRig>,
    attempts: u32,
    clips: HashMap<String, ClipState>,
    layout: Option<Layout>,
    pending: Option<(Handle<AnimationGraph>, Layout, u32)>,
    /// `RootMotionMode` is `Pin`: the hips stay over the capsule.
    pub pin_root_motion: bool,
    frames_bound: u32,
    warned: HashSet<String>,
    probe: Probe,
}

impl AnimatorRuntime {
    fn new(animator: InstanceId) -> Self {
        Self {
            animator,
            rig: None,
            attempts: 0,
            clips: HashMap::new(),
            layout: None,
            pending: None,
            pin_root_motion: true,
            frames_bound: 0,
            warned: HashSet::new(),
            probe: Probe::default(),
        }
    }

    /// The tracks this Animator draws this frame, with their weights, for
    /// the Player's agent loop and diagnostics.
    pub fn is_bound(&self) -> bool {
        self.rig.is_some()
    }
}

struct BoundRig {
    joints: RigJoints,
    rest: Handle<AnimationClip>,
    clips: Option<RigClips>,
    /// Skeleton rigs: the avatar root.
    avatar: Option<Entity>,
    skeleton_root: Option<Entity>,
    root_fixed: bool,
    /// The entity whose loss unbinds the rig.
    anchor: Entity,
    /// Part rigs: joint indices from the root outward.
    order: Vec<usize>,
    /// The joints answer to this Animator. A skeleton is taken over once one
    /// of its tracks plays; until then the avatar's own motion graph keeps
    /// animating it. A part rig is taken at bind.
    taken: bool,
}

enum ClipState {
    Loading { handle: Handle<AnimationClip>, file: Option<PathBuf>, name: String },
    Ready { handle: Handle<AnimationClip>, coverage: Vec<usize>, info: ClipInfo },
    Failed,
}

#[derive(Clone, PartialEq)]
struct Layout {
    members: Vec<(InstanceId, AssetId<AnimationClip>)>,
    classes: Vec<CoverageClass>,
    nodes: Vec<ClassNodes>,
}

#[derive(Default)]
struct Probe {
    frames: u32,
    snapshot: Vec<(Entity, Quat)>,
    peak_deg: f32,
    reported: bool,
}

// ============================================================================
// Update: the track clock
// ============================================================================

/// Advance every track to the clock, raising their events. The shell places
/// it after its clock advances and before its scripts.
pub fn step_tracks(tree: Option<Res<LiveTree>>) {
    if let Some(tree) = tree {
        let mut g = tree.dm.lock();
        if g.animation.space_root() != tree.space_root.as_deref() {
            g.animation.set_space_root(tree.space_root.clone());
        }
        g.step_animation();
    }
}

// ============================================================================
// Update: bind, load, drive
// ============================================================================

/// A skeleton's bind pose, the frame its rig binds.
pub fn capture_bind_poses(
    mut commands: Commands,
    rigs: Query<(Entity, &AvatarRig), Added<AvatarRig>>,
    transforms: Query<&Transform>,
) {
    for (entity, rig) in rigs.iter() {
        let pose = rig.by_key.values().filter_map(|e| transforms.get(*e).ok().map(|t| (*e, *t))).collect();
        commands.entity(entity).insert(BindPose(pose));
    }
}

/// One runtime entity per Animator in Workspace.
pub fn track_animators(
    mut commands: Commands,
    tree: Option<Res<LiveTree>>,
    mut index: ResMut<AnimatorIndex>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let Some(tree) = tree else {
        for (_, e) in index.by_animator.drain() {
            commands.entity(e).try_despawn();
        }
        index.tree = 0;
        return;
    };
    let id = std::sync::Arc::as_ptr(&tree.dm) as usize;
    if index.tree != id {
        for (_, e) in index.by_animator.drain() {
            commands.entity(e).try_despawn();
        }
        index.tree = id;
        index.structure = u64::MAX;
        index.frames_since_scan = SCAN_EVERY_FRAMES;
    }
    index.frames_since_scan = index.frames_since_scan.saturating_add(1);
    let live: HashSet<InstanceId> = {
        let g = tree.dm.lock();
        if g.structure_version == index.structure || index.frames_since_scan < SCAN_EVERY_FRAMES {
            return;
        }
        index.structure = g.structure_version;
        index.frames_since_scan = 0;
        g.ids_of_class("Animator").into_iter().filter(|a| g.in_workspace(*a)).collect()
    };
    let gone: Vec<InstanceId> = index.by_animator.keys().filter(|a| !live.contains(a)).copied().collect();
    for a in gone {
        if let Some(e) = index.by_animator.remove(&a) {
            commands.entity(e).try_despawn();
        }
    }
    for a in live {
        if index.by_animator.contains_key(&a) {
            continue;
        }
        let graph = graphs.add(AnimationGraph::new());
        let e = commands
            .spawn((Name::new("Animator"), AnimatorRuntime::new(a), AnimationPlayer::default(), AnimationGraphHandle(graph)))
            .id();
        index.by_animator.insert(a, e);
    }
}

/// The Model an Animator animates, and its root part.
fn rig_model(g: &DataModel, animator: InstanceId) -> Option<(InstanceId, Option<InstanceId>)> {
    let owner = g.parent(animator)?;
    let owner_class = g.class_of(owner)?;
    if !matches!(owner_class, "Humanoid" | "AnimationController") {
        return None;
    }
    let model = g.parent(owner)?;
    let root = match g.get_prop(model, "PrimaryPart") {
        Some(DmValue::Instance(p)) if g.exists(p) => Some(p),
        _ => g.find_first_child(model, "HumanoidRootPart", false),
    };
    Some((model, root))
}

/// A skeleton stays this Animator's while its character's root part is still
/// bound to the avatar. When the binding moves to another character, as when
/// a host's replicated character takes the local avatar, that character's
/// Animator drives the skeleton instead, never both.
fn root_still_stands_for(g: &DataModel, animator: InstanceId, rig: &BoundRig) -> bool {
    let Some(avatar) = rig.avatar else { return true };
    rig_model(g, animator).and_then(|(_, root)| root).and_then(|p| g.entity_of(p)) == Some(avatar.to_bits())
}

/// Bind each unbound Animator to its rig.
#[allow(clippy::too_many_arguments)]
pub fn bind_rigs(
    mut commands: Commands,
    tree: Option<Res<LiveTree>>,
    mut clip_assets: ResMut<Assets<AnimationClip>>,
    mut runtimes: Query<(Entity, &mut AnimatorRuntime)>,
    rigs: Query<(&AvatarRig, Option<&AvatarDescriptor>, Option<&BindPose>)>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    alive: Query<()>,
) {
    let Some(tree) = tree else { return };
    let g = tree.dm.lock();
    for (entity, mut rt) in runtimes.iter_mut() {
        if let Some(rig) = rt.rig.as_ref() {
            if alive.get(rig.anchor).is_ok() && root_still_stands_for(&g, rt.animator, rig) {
                continue;
            }
            // The body went away, or its binding moved to another character;
            // a new one binds afresh.
            rt.rig = None;
            rt.layout = None;
            rt.pending = None;
            rt.clips.clear();
        }
        rt.attempts = rt.attempts.wrapping_add(1);
        if rt.attempts % BIND_RETRY_FRAMES != 1 {
            continue;
        }
        let Some((model, root_part)) = rig_model(&g, rt.animator) else { continue };

        // A skeleton: the root part is bound to an avatar with a rig.
        let root_entity = root_part.and_then(|p| g.entity_of(p)).map(Entity::from_bits);
        if let Some(avatar) = root_entity {
            if let Ok((avatar_rig, descriptor, bind_pose)) = rigs.get(avatar) {
                if legacy_motion() {
                    continue;
                }
                let definition = descriptor.map(|d| d.resolved_rig());
                let joints = skeleton_joints(
                    avatar_rig,
                    avatar,
                    bind_pose,
                    &transforms,
                    &parents,
                    definition.as_ref().map(|d| d.bone_aliases.clone()).unwrap_or_default(),
                );
                let rest = clip_assets.add(rest_clip(&joints));
                let clips = definition.map(|d| RigClips { slots: d.animations.clone(), extra: Vec::new() });
                info!("animation: an Animator bound a skeleton of {} bones ({avatar:?})", joints.joints.len());
                rt.rig = Some(BoundRig {
                    joints,
                    rest,
                    clips,
                    avatar: Some(avatar),
                    skeleton_root: avatar_rig.skeleton_root,
                    root_fixed: false,
                    anchor: avatar,
                    order: Vec::new(),
                    taken: false,
                });
                rt.frames_bound = 0;
                continue;
            }
        }

        // Parts joined by Motor6Ds.
        if let Some((joints, order, anchor)) = motor_joints(&g, model, entity, &mut commands) {
            let rest = clip_assets.add(rest_clip(&joints));
            info!("animation: an Animator bound {} Motor6D joints", joints.joints.len());
            rt.rig = Some(BoundRig {
                joints,
                rest,
                clips: None,
                avatar: None,
                skeleton_root: None,
                root_fixed: true,
                anchor,
                order,
                taken: true,
            });
            rt.frames_bound = 0;
        }
    }
}

/// Whether `e` sits strictly below `ancestor`.
fn is_below(e: Entity, ancestor: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut at = e;
    for _ in 0..64 {
        match parents.get(at) {
            Ok(p) if p.parent() == ancestor => return true,
            Ok(p) => at = p.parent(),
            Err(_) => return false,
        }
    }
    false
}

/// The skeleton's bones as joints. The rig's names cover every named node its
/// bind walked, the avatar body ("Avatar") and its mesh holder included, so a
/// joint is a node strictly below the skeleton root. The body is never a
/// joint: animating it would hold the avatar at its bind-time position.
fn skeleton_joints(
    rig: &AvatarRig,
    avatar: Entity,
    bind_pose: Option<&BindPose>,
    transforms: &Query<&Transform>,
    parents: &Query<&ChildOf>,
    aliases: Vec<(String, String)>,
) -> RigJoints {
    let in_skeleton = |e: Entity| match rig.skeleton_root {
        Some(root) => is_below(e, root, parents),
        None => e != avatar && parents.get(e).map_or(true, |p| p.parent() != avatar),
    };
    let mut keys: Vec<(&String, &Entity)> = rig.by_key.iter().collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));
    let joints = keys
        .into_iter()
        .filter(|(_, e)| in_skeleton(**e))
        .map(|(key, e)| RigJoint {
            key: key.clone(),
            parent_key: None,
            target: skeleton_target(key),
            entity: *e,
            rest: bind_pose
                .and_then(|p| p.0.get(e).copied())
                .or_else(|| transforms.get(*e).ok().copied())
                .unwrap_or_default(),
        })
        .collect();
    RigJoints { kind: RigKind::Skeleton, joints, armature_scale: rig.armature_scale, aliases }
}

/// A Model's `Motor6D`s as joints, with a joint entity each (children of the
/// runtime entity, so they go with it). `None` until every enabled joint's
/// parts are drawn.
fn motor_joints(
    g: &DataModel,
    model: InstanceId,
    runtime: Entity,
    commands: &mut Commands,
) -> Option<(RigJoints, Vec<usize>, Entity)> {
    struct Found {
        part0: InstanceId,
        part1: InstanceId,
        e0: Entity,
        e1: Entity,
        c0: Transform,
        c1: Transform,
        enabled: bool,
        name0: String,
        name1: String,
    }
    let mut found = Vec::new();
    for id in g.descendants(model) {
        if g.class_of(id) != Some("Motor6D") {
            continue;
        }
        let part = |name: &str| match g.get_prop(id, name) {
            Some(DmValue::Instance(p)) if g.exists(p) => Some(p),
            _ => None,
        };
        let (Some(part0), Some(part1)) = (part("Part0"), part("Part1")) else { continue };
        let entity = |p: InstanceId| g.entity_of(p).map(Entity::from_bits);
        let (Some(e0), Some(e1)) = (entity(part0), entity(part1)) else {
            // Not drawn yet: try again later.
            return None;
        };
        let cframe = |name: &str| match g.get_prop(id, name) {
            Some(DmValue::CFrame(cf)) => cf.to_transform(),
            _ => Transform::IDENTITY,
        };
        found.push(Found {
            part0,
            part1,
            e0,
            e1,
            c0: cframe("C0"),
            c1: cframe("C1"),
            enabled: g.get_prop(id, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true),
            name0: g.name_of(part0).unwrap_or_default().to_string(),
            name1: g.name_of(part1).unwrap_or_default().to_string(),
        });
    }
    if found.is_empty() {
        return None;
    }

    // From the root outward: a joint whose Part0 is no joint's Part1 is a root.
    let children: HashSet<InstanceId> = found.iter().map(|f| f.part1).collect();
    let mut order = Vec::with_capacity(found.len());
    let mut placed: HashSet<InstanceId> = found.iter().filter(|f| !children.contains(&f.part0)).map(|f| f.part0).collect();
    let anchor_part = found.iter().find(|f| !children.contains(&f.part0)).map(|f| f.e0)?;
    let mut remaining: Vec<usize> = (0..found.len()).collect();
    while !remaining.is_empty() {
        let before = remaining.len();
        remaining.retain(|&i| {
            if placed.contains(&found[i].part0) {
                order.push(i);
                placed.insert(found[i].part1);
                false
            } else {
                true
            }
        });
        if remaining.len() == before {
            // A cycle: leave those joints out rather than loop.
            break;
        }
    }

    let joints = found
        .iter()
        .map(|f| {
            let target = joint_target(&f.name0, &f.name1);
            let entity = commands
                .spawn((
                    Name::new(format!("Joint {}/{}", f.name0, f.name1)),
                    JointPose::default(),
                    Motor6DLink { part0: f.e0, part1: f.e1, c0: f.c0, c1: f.c1, enabled: f.enabled },
                    target,
                    AnimatedBy(runtime),
                    ChildOf(runtime),
                ))
                .id();
            RigJoint { key: f.name1.clone(), parent_key: Some(f.name0.clone()), target, entity, rest: Transform::IDENTITY }
        })
        .collect();
    Some((RigJoints { kind: RigKind::Parts, joints, armature_scale: 1.0, aliases: Vec::new() }, order, anchor_part))
}

/// Resolve and load every clip an Animator's tracks name, and report each
/// ready clip's facts to the track model.
#[allow(clippy::too_many_arguments)]
pub fn load_clips(
    mut commands: Commands,
    tree: Option<Res<LiveTree>>,
    asset_server: Res<AssetServer>,
    mut clip_assets: ResMut<Assets<AnimationClip>>,
    mut runtimes: Query<&mut AnimatorRuntime>,
    transforms: Query<&Transform>,
) {
    let Some(tree) = tree else { return };
    let bundled = crate::avatar::boot::bundled_root();
    let mut warnings: Vec<String> = Vec::new();

    // Resolve new contents while the tree is locked.
    {
        let g = tree.dm.lock();
        let roots = Roots { space: tree.space_root.as_deref(), bundled: &bundled };
        for mut rt in runtimes.iter_mut() {
            let rt = &mut *rt;
            let Some(rig) = rt.rig.as_ref() else { continue };
            let contents: Vec<String> = g
                .animation
                .tracks()
                .filter(|(_, t)| t.animator == rt.animator)
                .map(|(_, t)| t.content.clone())
                .collect();
            for content in contents {
                if rt.clips.contains_key(&content) {
                    continue;
                }
                let state = match content::resolve(&content, &g, rig.clips.as_ref(), &roots) {
                    Err(e) => {
                        warnings.push(e);
                        ClipState::Failed
                    }
                    Ok(ClipSource::Gltf { asset_path, file }) => {
                        if rig.joints.kind == RigKind::Skeleton {
                            ClipState::Loading { handle: asset_server.load(asset_path.clone()), file, name: asset_path }
                        } else {
                            warnings.push(format!(
                                "{content}: a glTF clip plays on a skeleton; this Animator's rig is Motor6D parts, \
                                 which play KeyframeSequences"
                            ));
                            ClipState::Failed
                        }
                    }
                    Ok(ClipSource::Record { file }) => {
                        ready_from_record(&file, &content, &rig.joints, &mut clip_assets, &mut warnings)
                    }
                    Ok(ClipSource::Live { instance }) => match sequence_from_tree(&g, instance) {
                        Some(sequence) => ready_from_sequence(&sequence, &content, &rig.joints, &mut clip_assets, &mut warnings),
                        // A sequence the tree holds without keyframes: its record on disk.
                        None => match record_path(&content, tree.space_root.as_deref()) {
                            Some(file) => ready_from_record(&file, &content, &rig.joints, &mut clip_assets, &mut warnings),
                            None => {
                                warnings.push(format!("{content}: the KeyframeSequence has no keyframes"));
                                ClipState::Failed
                            }
                        },
                    },
                };
                rt.clips.insert(content, state);
            }
        }
    }

    // Finish glTF loads.
    for mut rt in runtimes.iter_mut() {
        let rt = &mut *rt;
        let Some(rig) = rt.rig.as_mut() else { continue };
        for state in rt.clips.values_mut() {
            let ClipState::Loading { handle, file, name } = state else { continue };
            if let Some(bevy::asset::LoadState::Failed(e)) = asset_server.get_load_state(handle.id()) {
                warnings.push(format!("{name}: {e}"));
                *state = ClipState::Failed;
                continue;
            }
            let Some(source) = clip_assets.get(handle.id()).cloned() else { continue };
            match retarget_for_skeleton(&source, file.as_deref(), name, &rig.joints.aliases) {
                Ok((mut clip, root_fix)) => {
                    if !rig.root_fixed {
                        rig.root_fixed = true;
                        if let (Some(fix), Some(skel)) = (root_fix, rig.skeleton_root) {
                            let scale = transforms.get(skel).map(|t| t.scale).unwrap_or(Vec3::ONE);
                            commands.entity(skel).insert(Transform { rotation: fix, scale, ..default() });
                        }
                    }
                    let coverage = complete_skeleton_clip(&mut clip, &rig.joints);
                    if coverage.is_empty() {
                        warnings.push(format!("{name}: the clip animates none of this skeleton's bones"));
                        *state = ClipState::Failed;
                        continue;
                    }
                    let info = ClipInfo {
                        length: clip.duration(),
                        looped: true,
                        priority: Priority::Core,
                        keyframes: Vec::new(),
                        markers: Vec::new(),
                    };
                    let handle = clip_assets.add(clip);
                    *state = ClipState::Ready { handle, coverage, info };
                }
                Err(e) => {
                    warnings.push(format!("{name}: {e}"));
                    *state = ClipState::Failed;
                }
            }
        }
    }

    // Tell the track model what it needs to know about each ready clip.
    let mut g = tree.dm.lock();
    for rt in runtimes.iter() {
        for (content, state) in &rt.clips {
            if let ClipState::Ready { info, .. } = state {
                if g.clip_info(rt.animator, content).is_none() {
                    g.set_clip_info(rt.animator, content, info.clone());
                }
            }
        }
    }
    for w in warnings {
        g.print(OutputLevel::Warn, "Animator", w);
    }
}

fn record_path(content: &str, space_root: Option<&Path>) -> Option<PathBuf> {
    let rel = content::safe_relative(content.strip_prefix("space://")?)?;
    let file = space_root?.join(rel).join("_instance.toml");
    file.is_file().then_some(file)
}

fn ready_from_record(
    file: &Path,
    content: &str,
    rig: &RigJoints,
    clip_assets: &mut Assets<AnimationClip>,
    warnings: &mut Vec<String>,
) -> ClipState {
    let text = match std::fs::read(file) {
        Ok(bytes) if bytes.len() <= super::clip::MAX_RECORD_BYTES => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(bytes) => {
            warnings.push(format!("{content}: {} bytes is over the clip limit", bytes.len()));
            return ClipState::Failed;
        }
        Err(e) => {
            warnings.push(format!("{content}: {e}"));
            return ClipState::Failed;
        }
    };
    match parse_sequence(&text, &record_fallback_name(file)) {
        Ok((sequence, problems)) => {
            for p in problems {
                warnings.push(format!("{content}: {p}"));
            }
            ready_from_sequence(&sequence, content, rig, clip_assets, warnings)
        }
        Err(e) => {
            warnings.push(format!("{content}: {e}"));
            ClipState::Failed
        }
    }
}

fn ready_from_sequence(
    sequence: &Sequence,
    content: &str,
    rig: &RigJoints,
    clip_assets: &mut Assets<AnimationClip>,
    warnings: &mut Vec<String>,
) -> ClipState {
    let built = build_sequence_clip(sequence, rig);
    if !built.unmatched.is_empty() {
        let mut names = built.unmatched.clone();
        names.sort();
        names.dedup();
        warnings.push(format!(
            "{content}: poses {} joint(s) this rig does not have ({})",
            names.len(),
            names.join(", ")
        ));
    }
    ClipState::Ready { handle: clip_assets.add(built.clip), coverage: built.coverage, info: sequence.info() }
}

/// A private, retargeted copy of a glTF clip for a skeleton, and the rotation
/// the clip's own root carries (Mixamo clips are authored Z-up).
#[cfg(feature = "model-import")]
fn retarget_for_skeleton(
    source: &AnimationClip,
    file: Option<&Path>,
    name: &str,
    aliases: &[(String, String)],
) -> Result<(AnimationClip, Option<Quat>), String> {
    let file = file.ok_or("no file to read the clip's bone names from")?;
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let fix = crate::avatar::retarget::clip_root_rotation(&bytes);
    let mut clip = source.clone();
    crate::avatar::retarget::retarget_clip_with_aliases(&mut clip, &bytes, name, aliases)?;
    Ok((clip, fix))
}

#[cfg(not(feature = "model-import"))]
fn retarget_for_skeleton(
    _source: &AnimationClip,
    _file: Option<&Path>,
    _name: &str,
    _aliases: &[(String, String)],
) -> Result<(AnimationClip, Option<Quat>), String> {
    Err("glTF clips need the model-import feature".into())
}

/// Build each rig's graph from its tracks and write this frame's weights and
/// times into its player.
#[allow(clippy::type_complexity)]
pub fn drive_graphs(
    mut commands: Commands,
    tree: Option<Res<LiveTree>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut runtimes: Query<(Entity, &mut AnimatorRuntime, &mut AnimationPlayer, &mut AnimationGraphHandle)>,
    posed: Query<(), With<PoseReady>>,
) {
    let Some(tree) = tree else { return };
    let (frame, pins) = {
        let g = tree.dm.lock();
        let frame: Vec<AnimatorFrame> = g.animation_frame();
        let pins: HashMap<InstanceId, bool> = runtimes
            .iter()
            .map(|(_, rt, ..)| {
                let keep = matches!(
                    g.get_prop(rt.animator, "RootMotionMode").as_ref().and_then(|v| v.as_str()),
                    Some("Keep")
                );
                (rt.animator, !keep)
            })
            .collect();
        (frame, pins)
    };

    for (entity, mut rt, mut player, mut graph_handle) in runtimes.iter_mut() {
        let rt = &mut *rt;
        let Some(rig) = rt.rig.as_ref() else { continue };
        rt.frames_bound = rt.frames_bound.saturating_add(1);
        rt.pin_root_motion = pins.get(&rt.animator).copied().unwrap_or(true);

        // The tracks whose clips are ready, in track order.
        let empty = Vec::new();
        let tracks: &Vec<TrackSample> =
            frame.iter().find(|f| f.animator == rt.animator).map(|f| &f.tracks).unwrap_or(&empty);
        let mut samples: Vec<&TrackSample> = Vec::new();
        let mut handles: Vec<Handle<AnimationClip>> = Vec::new();
        let mut coverages: Vec<Vec<usize>> = Vec::new();
        for s in tracks {
            if let Some(ClipState::Ready { handle, coverage, .. }) = rt.clips.get(&s.content) {
                samples.push(s);
                handles.push(handle.clone());
                coverages.push(coverage.clone());
            }
        }
        let members: Vec<(InstanceId, AssetId<AnimationClip>)> =
            samples.iter().zip(&handles).map(|(s, h)| (s.track, h.id())).collect();

        // A new membership builds a new graph, swapped in once Bevy has threaded it.
        let current = rt.layout.as_ref().map(|l| &l.members);
        let pending = rt.pending.as_ref().map(|(_, l, _)| &l.members);
        if current != Some(&members) && pending != Some(&members) {
            let (classes, merged) = coverage_classes(rig.joints.joints.len(), &coverages);
            if merged && rt.warned.insert("merged".into()) {
                warn!("animation: a rig has more joint classes than Bevy's 64 mask groups; its blend is approximate");
            }
            let targets: Vec<AnimationTargetId> = rig.joints.joints.iter().map(|j| j.target).collect();
            let (graph, nodes) = build_graph(&rig.rest, &handles, &classes, &targets);
            let handle = graphs.add(graph);
            rt.pending = Some((handle, Layout { members: members.clone(), classes, nodes }, 0));
        }
        if let Some((_, _, frames)) = rt.pending.as_mut() {
            *frames += 1;
        }
        if rt.pending.as_ref().is_some_and(|(_, _, f)| *f >= GRAPH_SWAP_FRAMES) {
            let (handle, layout, _) = rt.pending.take().expect("pending");
            graph_handle.0 = handle;
            player.stop_all();
            for n in &layout.nodes {
                player.start(n.rest).pause();
                for (_, node) in &n.tracks {
                    player.start(*node).pause();
                }
            }
            rt.layout = Some(layout);
        }

        // This frame's weights and times, on the graph in use.
        if let Some(layout) = rt.layout.as_ref() {
            let index_of: HashMap<InstanceId, usize> =
                samples.iter().enumerate().map(|(i, s)| (s.track, i)).collect();
            for (class, nodes) in layout.classes.iter().zip(&layout.nodes) {
                // The layout's track indices refer to its own members; map them
                // to this frame's samples by track id.
                let current: Vec<Option<&TrackSample>> = class
                    .tracks
                    .iter()
                    .map(|&t| layout.members.get(t).and_then(|(id, _)| index_of.get(id)).map(|&i| samples[i]))
                    .collect();
                let pairs: Vec<(Priority, f32)> =
                    current.iter().map(|s| s.map_or((Priority::Core, 0.0), |s| (s.priority, s.weight))).collect();
                let (weights, rest) = effective_weights(&pairs);
                if let Some(a) = player.animation_mut(nodes.rest) {
                    a.set_weight(rest).set_seek_time(0.0);
                }
                for (k, (_, node)) in nodes.tracks.iter().enumerate() {
                    let (w, t) = match current.get(k).copied().flatten() {
                        Some(s) => (weights.get(k).copied().unwrap_or(0.0), s.time),
                        None => (0.0, 0.0),
                    };
                    if let Some(a) = player.animation_mut(*node) {
                        a.set_weight(w).set_seek_time(t);
                    }
                }
            }
        }

        // The feet calibrate once the pose is the animated one, or once a
        // rig has played nothing for a while.
        if let Some(avatar) = rig.avatar {
            let animated = rt.layout.as_ref().is_some_and(|l| !l.members.is_empty());
            if !posed.contains(avatar) && (animated || rt.frames_bound > POSE_READY_FALLBACK_FRAMES) {
                commands.entity(avatar).insert(PoseReady);
            }
        }

        // A skeleton's bones answer to this Animator once it plays something.
        // Until then the avatar's own motion graph keeps animating it, so an
        // avatar never stands in its rest pose waiting for tracks or clips.
        let playing = rt.layout.as_ref().is_some_and(|l| !l.members.is_empty());
        if let Some(rig) = rt.rig.as_mut().filter(|r| playing && !r.taken) {
            for j in &rig.joints.joints {
                commands.entity(j.entity).insert((j.target, AnimatedBy(entity)));
            }
            if let Some(avatar) = rig.avatar {
                // The avatar's own motion graph lets these bones go.
                commands.entity(avatar).insert(AnimatorDriven);
                info!(
                    "animation: an Animator took over a skeleton of {} bones from the avatar's own motion graph ({avatar:?})",
                    rig.joints.joints.len()
                );
            }
            rig.taken = true;
        }
    }
}

/// A rig no Animator binds is posed after a while anyway. Its motion graph
/// says so sooner, when it has one (`avatar::spawn`'s `PoseFinal`).
pub fn pose_ready_fallback(
    mut commands: Commands,
    rigs: Query<Entity, (With<AvatarRig>, Without<PoseReady>)>,
    mut waited: Local<HashMap<Entity, u32>>,
) {
    waited.retain(|e, _| rigs.contains(*e));
    for e in rigs.iter() {
        let frames = waited.entry(e).or_insert(0);
        *frames += 1;
        if *frames > POSE_READY_FALLBACK_FRAMES * 2 {
            commands.entity(e).insert(PoseReady);
        }
    }
}

// ============================================================================
// PostUpdate: after Bevy samples the graphs
// ============================================================================

/// `RootMotionMode` `Pin`: the hips translation returns to the bind pose, so
/// a clip's root motion cannot walk the body off its capsule.
pub fn pin_root_motion(
    runtimes: Query<&AnimatorRuntime>,
    rigs: Query<&AvatarRig>,
    mut bones: Query<&mut Transform>,
) {
    for rt in runtimes.iter() {
        let Some(rig) = rt.rig.as_ref() else { continue };
        if !rt.pin_root_motion || rig.joints.kind != RigKind::Skeleton {
            continue;
        }
        if rt.layout.as_ref().map_or(true, |l| l.members.is_empty()) {
            continue;
        }
        let Some(avatar_rig) = rig.avatar.and_then(|a| rigs.get(a).ok()) else { continue };
        let Some(hips) = avatar_rig.bone(HumanoidBone::Hips) else { continue };
        let pinned = match avatar_rig.skeleton_root.and_then(|s| bones.get(s).ok().copied()) {
            Some(root) if root.scale.abs().min_element() > 1e-6 => {
                root.rotation.inverse() * (avatar_rig.bind_hips_in_root_parent - root.translation) / root.scale
            }
            _ => avatar_rig.bind_hips_translation,
        };
        if let Ok(mut t) = bones.get_mut(hips) {
            t.translation = pinned;
        }
    }
}

/// Part rigs: from the root outward, each `Part1` takes
/// `Part0 * C0 * Transform * C1:Inverse()`, keeping its own size.
pub fn motor_forward_pass(
    runtimes: Query<&AnimatorRuntime>,
    links: Query<(&JointPose, &Motor6DLink)>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    for rt in runtimes.iter() {
        let Some(rig) = rt.rig.as_ref() else { continue };
        if rig.joints.kind != RigKind::Parts {
            continue;
        }
        for &j in &rig.order {
            let Some(joint) = rig.joints.joints.get(j) else { continue };
            let Ok((pose, link)) = links.get(joint.entity) else { continue };
            if !link.enabled {
                continue;
            }
            let Some(part0) = world_affine(link.part0, &parents, &transforms) else { continue };
            let (_, r0, t0) = part0.to_scale_rotation_translation();
            let world = Transform::from_translation(t0).with_rotation(r0)
                * link.c0
                * Transform::from_translation(pose.translation).with_rotation(pose.rotation)
                * inverse_rigid(&link.c1);
            let parent_affine = parents
                .get(link.part1)
                .ok()
                .and_then(|c| world_affine(c.parent(), &parents, &transforms))
                .unwrap_or(Mat4::IDENTITY);
            let Ok(mut t) = transforms.get_mut(link.part1) else { continue };
            let desired = Mat4::from_scale_rotation_translation(t.scale, world.rotation, world.translation);
            let local = parent_affine.inverse() * desired;
            let (_, rotation, translation) = local.to_scale_rotation_translation();
            t.translation = translation;
            t.rotation = rotation;
        }
    }
}

fn inverse_rigid(t: &Transform) -> Transform {
    let rotation = t.rotation.inverse();
    Transform::from_translation(rotation * -t.translation).with_rotation(rotation)
}

/// An entity's world transform from its local transforms, this frame (the
/// propagated `GlobalTransform` is a frame behind in `PostUpdate`).
fn world_affine(entity: Entity, parents: &Query<&ChildOf>, transforms: &Query<&mut Transform>) -> Option<Mat4> {
    let mut m = transforms.get(entity).ok()?.to_matrix();
    let mut cur = entity;
    let mut guard = 0;
    while let Ok(c) = parents.get(cur) {
        guard += 1;
        if guard > 64 {
            break;
        }
        cur = c.parent();
        if let Ok(t) = transforms.get(cur) {
            m = t.to_matrix() * m;
        }
    }
    Some(m)
}

/// Is a skeleton's pose continuously changing? Reports once per rig, a few
/// seconds after it starts playing: the one animation signal that can fail.
pub fn probe_liveness(mut runtimes: Query<&mut AnimatorRuntime>, bones: Query<&Transform>) {
    for mut rt in runtimes.iter_mut() {
        let rt = &mut *rt;
        if rt.probe.reported {
            continue;
        }
        let Some(rig) = rt.rig.as_ref() else { continue };
        if rig.joints.kind != RigKind::Skeleton || rt.layout.as_ref().map_or(true, |l| l.members.is_empty()) {
            continue;
        }
        rt.probe.frames += 1;
        // Skip the settle into the first animated pose.
        if rt.probe.frames == 30 {
            rt.probe.snapshot =
                rig.joints.joints.iter().filter_map(|j| bones.get(j.entity).ok().map(|t| (j.entity, t.rotation))).collect();
            continue;
        }
        for (e, start) in &rt.probe.snapshot {
            if let Ok(now) = bones.get(*e) {
                rt.probe.peak_deg = rt.probe.peak_deg.max(start.angle_between(now.rotation).to_degrees());
            }
        }
        if rt.probe.frames < 270 {
            continue;
        }
        rt.probe.reported = true;
        if rt.probe.peak_deg > 0.05 {
            info!("animation: LIVE, bones moved {:.2} degrees in steady state", rt.probe.peak_deg);
        } else {
            warn!("animation: FROZEN, bones moved {:.4} degrees over four seconds of playback", rt.probe.peak_deg);
        }
    }
}

/// The tracks a skeleton rig drew last frame: name, weight, time. The
/// Player's agent loop reads this for its observation.
pub fn drawn_tracks(g: &DataModel, animator: InstanceId) -> Vec<(String, f32, f32)> {
    g.animation_frame()
        .into_iter()
        .filter(|f| f.animator == animator)
        .flat_map(|f| f.tracks)
        .map(|s| (g.name_of(s.track).unwrap_or_default().to_string(), s.weight, s.time))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rig's names include the avatar body ("Avatar") and its mesh holder
    /// ("AvatarMesh"), but only the skeleton's own nodes become joints: a body
    /// animated to its rest pose was held at its bind-time position.
    #[test]
    fn only_the_skeleton_s_nodes_become_joints_never_the_body() {
        use bevy::ecs::system::SystemState;
        let mut world = World::new();
        let avatar = world.spawn((Name::new("Avatar"), Transform::from_xyz(11.0, 1.313, 11.0))).id();
        let mesh = world.spawn((Name::new("AvatarMesh"), Transform::default(), ChildOf(avatar))).id();
        let armature = world.spawn((Name::new("Armature"), Transform::default(), ChildOf(mesh))).id();
        let hips = world.spawn((Name::new("mixamorig:Hips"), Transform::default(), ChildOf(armature))).id();
        let spine = world.spawn((Name::new("mixamorig:Spine"), Transform::default(), ChildOf(hips))).id();
        let mut rig = AvatarRig {
            bones: Default::default(),
            by_key: [("avatar", avatar), ("avatarmesh", mesh), ("armature", armature), ("hips", hips), ("spine", spine)]
                .into_iter()
                .map(|(k, e)| (k.to_string(), e))
                .collect(),
            skeleton_root: Some(armature),
            unresolved: Vec::new(),
            bind_height_m: 1.8,
            foot_half_separation: 0.1,
            bind_hip_height: 1.0,
            armature_scale: 1.0,
            bind_hips_translation: Vec3::ZERO,
            bind_hips_in_root_parent: Vec3::ZERO,
            signature: 0,
        };
        let mut state: SystemState<(Query<&Transform>, Query<&ChildOf>)> = SystemState::new(&mut world);
        let (transforms, parents) = state.get(&world).expect("both queries are valid on this world");
        let joined = |rig: &AvatarRig| {
            let mut v: Vec<Entity> =
                skeleton_joints(rig, avatar, None, &transforms, &parents, Vec::new()).joints.iter().map(|j| j.entity).collect();
            v.sort();
            v
        };
        let mut expected = vec![hips, spine];
        expected.sort();
        assert_eq!(joined(&rig), expected, "only nodes below the skeleton root");

        // Without a skeleton root, the body and its mesh holder still stay out.
        rig.skeleton_root = None;
        let mut expected = vec![armature, hips, spine];
        expected.sort();
        assert_eq!(joined(&rig), expected);
    }

    #[test]
    fn a_humanoid_or_controller_names_the_model_and_its_root() {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let model = g.create_virtual("Model", "Npc", Some(ws));
        let root = g.create_virtual("Part", "HumanoidRootPart", Some(model));
        let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(model));
        let animator = g.create_virtual("Animator", "Animator", Some(humanoid));
        assert_eq!(rig_model(&g, animator), Some((model, Some(root))));
        let loose = g.create_virtual("Animator", "Animator", Some(ws));
        assert_eq!(rig_model(&g, loose), None, "an Animator in Workspace animates nothing");
    }

    #[test]
    fn the_rigid_inverse_undoes_a_joint_frame() {
        let c1 = Transform::from_xyz(0.2, -0.5, 0.1).with_rotation(Quat::from_rotation_y(0.7));
        let round = c1 * inverse_rigid(&c1);
        assert!(round.translation.length() < 1e-5);
        assert!(crate::animation::clip::rotation_gap(round.rotation, Quat::IDENTITY) < 1e-5);
    }
}

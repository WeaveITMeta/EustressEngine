//! # The matcher beside the gait
//!
//! A `MotionMatcher` in `Shadow` mode watches a character without moving it.
//! Each frame it measures the character's feet and the path it is about to
//! take, searches its `MotionDatabase` as the matcher would, and reports the
//! clip it would pick: in the Output when `Debug` is on, and always in the
//! read-only `CurrentClip`, `CurrentTime` and `LastCost` properties. The
//! `Animate` script keeps playing the gait, so the matcher's choices can be
//! compared with it before it is allowed to drive anything.
//!
//! The database is built from the clips the Animator already loaded for the
//! character: a `MotionDatabase`'s `Animation` children name them by their
//! `AnimationId` (`rig://walk`), and a clip counts once the Animate script has
//! started a track on it. The build waits until the avatar's feet are
//! calibrated to the ground, so the live and stored frames are measured against
//! the same ground.
//!
//! Everything that is arithmetic is a plain function here ([`build_skeleton`],
//! [`planar_state`], [`desired`], [`live_query`], [`build_database`]), tested
//! without a scene; [`shadow_matchers`] only gathers the inputs.

use std::collections::{HashMap, HashSet};

use bevy::animation::{AnimationClip, AnimationTargetId};
use bevy::math::{Mat4, Quat, Vec2, Vec3};
use bevy::prelude::*;
use tracing::{info, warn};

use super::database::Database;
use super::features::{FeatureLayout, FeatureWeights, Query as MatchQuery};
use super::sample::{
    clip_frames, reference_from_rest, ClipSpec, HipsMode, Landmarks, Reference, ShapeJoint, SkeletonShape,
};
use super::search::{MatchStep, Matcher, SearchConfig, TagFilter};
use super::trajectory::{predict_trajectory, to_local, wrap_angle, Desired, PlanarState, TrajectoryPoint, DEFAULT_TRAJECTORY_TIMES};
use crate::animation::runtime::{AnimatorIndex, AnimatorRuntime};
use crate::animation::LiveTree;
use crate::avatar::control::AvatarScriptControl;
use crate::avatar::rig::{AvatarRig, HumanoidBone};
use crate::avatar::spawn::{AvatarBody, AvatarIntent, PendingFootCalibration};
use crate::datamodel::{DataModel, DmValue, InstanceId};

/// Seconds between writes of the read-only time and cost properties.
const READOUT_EVERY: f32 = 0.1;

/// Frames to wait before building a database again after it failed.
const RETRY_FRAMES: u32 = 120;

// ── Settings, read from the tree ─────────────────────────────────────────

/// `MotionMatcher.Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatcherMode {
    /// The `Animate` script's gait plays and the matcher does nothing.
    #[default]
    Off,
    /// The matcher searches and reports; it moves nothing.
    Shadow,
    /// Reserved for the matcher that drives the pose. Runs as `Shadow` for now.
    Drive,
}

impl MatcherMode {
    pub fn parse(text: &str) -> Self {
        match text.trim().to_ascii_lowercase().as_str() {
            "shadow" => Self::Shadow,
            "drive" => Self::Drive,
            _ => Self::Off,
        }
    }
}

/// A `MotionMatcher`'s properties.
#[derive(Debug, Clone, PartialEq)]
pub struct MatcherSettings {
    pub mode: MatcherMode,
    /// The `MotionDatabase` it searches, when the property names one.
    pub database: Option<InstanceId>,
    pub search: SearchConfig,
    /// The half-life, seconds, of the spring that predicts the path.
    pub responsiveness: f32,
    pub debug: bool,
}

fn number(g: &DataModel, id: InstanceId, name: &str, default: f32) -> f32 {
    g.get_prop(id, name).and_then(|v| v.as_number()).map(|v| v as f32).filter(|v| v.is_finite()).unwrap_or(default)
}

impl MatcherSettings {
    pub fn read(g: &DataModel, id: InstanceId) -> Self {
        let mode = g.get_prop(id, "Mode").and_then(|v| v.as_str().map(MatcherMode::parse)).unwrap_or_default();
        let database = g
            .get_prop(id, "Database")
            .and_then(|v| v.as_instance())
            .filter(|d| g.exists(*d) && g.class_of(*d) == Some("MotionDatabase"));
        Self {
            mode,
            database,
            search: SearchConfig {
                interval: number(g, id, "SearchInterval", 0.1).max(0.01),
                blend_time: number(g, id, "BlendTime", 0.2).max(0.02),
                min_dwell: number(g, id, "MinDwell", 0.15).max(0.0),
                hysteresis: number(g, id, "Hysteresis", 0.1).clamp(0.0, 1.0),
                ..SearchConfig::default()
            },
            responsiveness: number(g, id, "Responsiveness", 0.15).max(0.02),
            debug: g.get_prop(id, "Debug").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }
}

/// One `Animation` child of a `MotionDatabase`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipSetting {
    /// The `Animation`'s name.
    pub name: String,
    /// Its `AnimationId`, which the Animator resolves (`rig://walk`).
    pub content: String,
    pub spec: ClipSpec,
}

/// A `MotionDatabase`'s properties and clips.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseSettings {
    pub times: Vec<f32>,
    pub weights: FeatureWeights,
    pub clips: Vec<ClipSetting>,
}

/// Seconds ahead, from a comma list: `None` unless it is positive, ascending
/// and short enough to be a path.
pub fn parse_times(text: &str) -> Option<Vec<f32>> {
    let times: Vec<f32> = text.split(',').map(|t| t.trim().parse::<f32>()).collect::<Result<_, _>>().ok()?;
    let sound = !times.is_empty()
        && times.len() <= 16
        && times.iter().all(|t| t.is_finite() && *t > 0.0)
        && times.windows(2).all(|w| w[1] > w[0]);
    sound.then_some(times)
}

impl DatabaseSettings {
    pub fn read(g: &DataModel, id: InstanceId) -> Self {
        let sample_rate = number(g, id, "SampleRate", 30.0).clamp(5.0, 120.0);
        let times = g
            .get_prop(id, "TrajectoryTimes")
            .and_then(|v| v.as_str().and_then(parse_times))
            .unwrap_or_else(|| DEFAULT_TRAJECTORY_TIMES.to_vec());
        let weights = FeatureWeights::from_class(
            number(g, id, "TrajectoryWeight", 1.0),
            number(g, id, "FacingWeight", 1.0),
            number(g, id, "PoseWeight", 0.7),
            number(g, id, "VelocityWeight", 0.5),
        );
        let attribute = |child: InstanceId, name: &str| g.get_attribute(child, name);
        let clips = g
            .children(id)
            .iter()
            .copied()
            .filter(|c| g.class_of(*c) == Some("Animation"))
            .map(|child| {
                let tags = attribute(child, "Tags")
                    .and_then(|v| v.as_str().map(str::to_string))
                    .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default();
                let hips = match attribute(child, "HipsMode").and_then(|v| v.as_str().map(str::to_ascii_lowercase)) {
                    Some(mode) if mode == "authored" => HipsMode::Authored,
                    _ => HipsMode::PinnedToRest,
                };
                let value = |name: &str| attribute(child, name).and_then(|v| v.as_number()).map_or(0.0, |v| v as f32);
                let name = g.name_of(child).unwrap_or("clip").to_string();
                ClipSetting {
                    content: g.get_prop(child, "AnimationId").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
                    spec: ClipSpec {
                        name: name.clone(),
                        tags,
                        fps: sample_rate,
                        looping: attribute(child, "Loop").and_then(|v| v.as_bool()).unwrap_or(true),
                        hips,
                        speed: value("Speed"),
                        heading_degrees: value("Heading"),
                        yaw_rate_degrees: value("YawRate"),
                    },
                    name,
                }
            })
            .collect();
        Self { times, weights, clips }
    }
}

// ── The skeleton, from the live rig ──────────────────────────────────────

/// A live rig's joints as the sampler wants them, parents first.
#[derive(Debug, Clone)]
pub struct LiveSkeleton {
    pub shape: SkeletonShape,
    pub landmarks: Landmarks,
    /// The entity of each joint of `shape`.
    pub entities: Vec<Entity>,
}

/// The joints of `joints` ordered parents first, each parent being the nearest
/// ancestor (by `parent_of`) that is itself a joint, with the three landmark
/// entities found among them. `root` is the transform from the skeleton root
/// to the character's frame (the avatar), which sits above the first joints.
pub fn build_skeleton(
    joints: &[(Entity, AnimationTargetId, Transform)],
    parent_of: &dyn Fn(Entity) -> Option<Entity>,
    hips: Entity,
    left_foot: Entity,
    right_foot: Entity,
    root: Transform,
) -> Result<LiveSkeleton, String> {
    let at: HashMap<Entity, usize> = joints.iter().enumerate().map(|(i, j)| (j.0, i)).collect();
    // The nearest ancestor that is a joint.
    let joint_parent: Vec<Option<usize>> = joints
        .iter()
        .map(|(entity, ..)| {
            let mut probe = *entity;
            for _ in 0..256 {
                match parent_of(probe) {
                    Some(p) => match at.get(&p) {
                        Some(&i) => return Some(i),
                        None => probe = p,
                    },
                    None => return None,
                }
            }
            None
        })
        .collect();
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = joint_parent[i] {
            d += 1;
            i = p;
            if d > joints.len() {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..joints.len()).collect();
    order.sort_by_key(|&i| (depth(i), i));
    let new_index: HashMap<usize, usize> = order.iter().enumerate().map(|(new, &old)| (old, new)).collect();
    let shape = SkeletonShape {
        joints: order
            .iter()
            .map(|&old| ShapeJoint {
                target: joints[old].1,
                parent: joint_parent[old].map(|p| new_index[&p]),
                rest: joints[old].2,
            })
            .collect(),
        root,
    };
    shape.validate()?;
    let find = |name: &str, entity: Entity| {
        at.get(&entity).map(|old| new_index[old]).ok_or_else(|| format!("the {name} is not one of the rig's joints"))
    };
    let landmarks = Landmarks { hips: find("hips", hips)?, left_foot: find("left foot", left_foot)?, right_foot: find("right foot", right_foot)? };
    Ok(LiveSkeleton { shape, landmarks, entities: order.iter().map(|&old| joints[old].0).collect() })
}

/// The database of `settings`' clips, sampled through `skeleton`. `clips` maps
/// each clip's `AnimationId` to the clip itself.
pub fn build_database(
    settings: &DatabaseSettings,
    skeleton: &LiveSkeleton,
    reference: &Reference,
    clips: &HashMap<String, &AnimationClip>,
) -> Result<Database, String> {
    let mut frames = Vec::with_capacity(settings.clips.len());
    for setting in &settings.clips {
        let clip = clips
            .get(&setting.content)
            .ok_or_else(|| format!("clip {:?}: {:?} is not loaded", setting.name, setting.content))?;
        frames.push(clip_frames(clip, &skeleton.shape, skeleton.landmarks, reference, &setting.spec)?);
    }
    Database::build(&frames, FeatureLayout::new(settings.times.len()), &settings.times, settings.weights)
}

// ── What the character is doing, in numbers ──────────────────────────────

/// What was measured last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Previous {
    pub position: Vec3,
    pub yaw: f32,
    pub left_foot: Vec3,
    pub right_foot: Vec3,
    pub hips_height: f32,
}

/// The feet and hips measured in the character's frame this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    pub left_foot: Vec3,
    pub right_foot: Vec3,
    pub hips_height: f32,
}

/// What the controller is asked to do, for a character that has intent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// World direction of travel; unit, or nearly zero for none.
    pub direction: Vec3,
    pub sprint: bool,
    pub walk_speed: f32,
    /// The capped sprint speed.
    pub sprint_speed: f32,
    /// A script's facing, when it owns it.
    pub script_yaw: Option<f32>,
}

/// The character's planar state: its velocity and turn rate by the change in
/// its position and yaw since last frame. The acceleration is left at zero,
/// since a difference of differences is noise at a frame's length.
pub fn planar_state(position: Vec3, yaw: f32, previous: Option<&Previous>, dt: f32) -> PlanarState {
    let here = Vec2::new(position.x, position.z);
    match previous {
        Some(p) if dt > 1.0e-4 => PlanarState {
            position: here,
            velocity: (here - Vec2::new(p.position.x, p.position.z)) / dt,
            acceleration: Vec2::ZERO,
            yaw,
            yaw_rate: wrap_angle(yaw - p.yaw) / dt,
        },
        _ => PlanarState { position: here, yaw, ..Default::default() },
    }
}

/// Where the character is being sent, and which way it should face. A
/// character without controls (another player's replica) is taken to want what
/// it is doing.
pub fn desired(controls: Option<&Controls>, state: &PlanarState) -> Desired {
    let Some(c) = controls else {
        return Desired { velocity: state.velocity, yaw: state.yaw };
    };
    let direction = Vec2::new(c.direction.x, c.direction.z);
    let moving = direction.length_squared() > 1.0e-4;
    let speed = if c.sprint { c.sprint_speed } else { c.walk_speed };
    let velocity = if moving { direction.normalize() * speed } else { Vec2::ZERO };
    let yaw = c.script_yaw.unwrap_or(if moving { (-direction.x).atan2(-direction.y) } else { state.yaw });
    Desired { velocity, yaw }
}

/// The query for this frame: the predicted path, the feet, and the speeds in
/// the character's frame.
pub fn live_query(
    trajectory: Vec<TrajectoryPoint>,
    measured: &Measured,
    previous: Option<&Previous>,
    state: &PlanarState,
    dt: f32,
) -> MatchQuery {
    let rate = |now: Vec3, before: Option<Vec3>| match before {
        Some(b) if dt > 1.0e-4 => (now - b) / dt,
        _ => Vec3::ZERO,
    };
    let local = to_local(state.velocity, state.yaw);
    let hips_velocity = match previous {
        Some(p) if dt > 1.0e-4 => (measured.hips_height - p.hips_height) / dt,
        _ => 0.0,
    };
    MatchQuery {
        trajectory,
        left_foot: measured.left_foot,
        right_foot: measured.right_foot,
        left_foot_velocity: rate(measured.left_foot, previous.map(|p| p.left_foot)),
        right_foot_velocity: rate(measured.right_foot, previous.map(|p| p.right_foot)),
        root_velocity: Vec3::new(local.x, hips_velocity, local.y),
    }
}

// ── The system ───────────────────────────────────────────────────────────

struct Built {
    db: Database,
    skeleton: LiveSkeleton,
    reference: Reference,
    settings: DatabaseSettings,
    avatar: Entity,
}

struct Shadow {
    built: Option<Built>,
    matcher: Matcher,
    previous: Option<Previous>,
    /// Things already said, so a repeated state is said once.
    said: HashSet<String>,
    retry_in: u32,
    since_readout: f32,
    clip: Option<usize>,
}

impl Shadow {
    fn new(config: SearchConfig, phase: f32) -> Self {
        Self {
            built: None,
            matcher: Matcher::new(config).with_phase(phase),
            previous: None,
            said: HashSet::new(),
            retry_in: 0,
            since_readout: READOUT_EVERY,
            clip: None,
        }
    }

    /// `true` the first time `what` is said.
    fn first(&mut self, what: &str) -> bool {
        self.said.insert(what.to_string())
    }
}

/// Every matcher in `Shadow` (or `Drive`) mode, by the matcher's instance.
#[derive(Resource, Default)]
pub struct ShadowMatchers {
    session: u64,
    by_matcher: HashMap<InstanceId, Shadow>,
}

struct Job {
    id: InstanceId,
    settings: MatcherSettings,
    database: Option<DatabaseSettings>,
    animator: Option<InstanceId>,
}

fn jobs(g: &DataModel) -> Vec<Job> {
    g.ids_of_class("MotionMatcher")
        .into_iter()
        .filter(|id| g.in_workspace(*id))
        .filter_map(|id| {
            let settings = MatcherSettings::read(g, id);
            if settings.mode == MatcherMode::Off {
                return None;
            }
            let database = settings.database.map(|d| DatabaseSettings::read(g, d));
            let animator = g
                .parent(id)
                .filter(|h| matches!(g.class_of(*h), Some("Humanoid" | "AnimationController")))
                .and_then(|h| g.find_first_child_of_class(h, "Animator", false));
            Some(Job { id, settings, database, animator })
        })
        .collect()
}

/// A stable 0..1 from an instance, to stagger one matcher's searches from
/// the next.
fn phase_of(id: InstanceId) -> f32 {
    // The top ten bits of a multiplicative hash.
    (id.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 54) as f32 / 1024.0
}

struct Readout {
    id: InstanceId,
    clip: String,
    time: f32,
    cost: f32,
}

/// Run every shadow matcher one frame.
#[allow(clippy::type_complexity)]
pub fn shadow_matchers(
    time: Res<Time>,
    tree: Option<Res<LiveTree>>,
    index: Res<AnimatorIndex>,
    runtimes: Query<&AnimatorRuntime>,
    clip_assets: Res<Assets<AnimationClip>>,
    avatars: Query<(
        &GlobalTransform,
        &Transform,
        &AvatarBody,
        Option<&AvatarIntent>,
        Option<&AvatarScriptControl>,
        &AvatarRig,
        Has<PendingFootCalibration>,
    )>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    mut state: ResMut<ShadowMatchers>,
) {
    let Some(tree) = tree else {
        state.by_matcher.clear();
        return;
    };
    // The session number, taken before the tree is locked: it locks the tree.
    let session = crate::datamodel::session_of(&tree.dm);
    if state.session != session {
        state.session = session;
        state.by_matcher.clear();
    }
    let jobs = jobs(&tree.dm.lock());
    state.by_matcher.retain(|id, _| jobs.iter().any(|j| j.id == *id));
    if jobs.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let mut readouts: Vec<Readout> = Vec::new();
    // The size of each database built this frame: its id, clips and frames.
    let mut sizes: Vec<(InstanceId, usize, usize)> = Vec::new();

    for job in &jobs {
        let shadow = state.by_matcher.entry(job.id).or_insert_with(|| Shadow::new(job.settings.search, phase_of(job.id)));
        shadow.matcher.set_config(job.settings.search);
        let Some(database) = job.database.as_ref() else {
            if shadow.first("no database") {
                info!("motion matcher: its Database property names no MotionDatabase");
            }
            continue;
        };
        let Some(animator) = job.animator else {
            if shadow.first("no animator") {
                info!("motion matcher: it must sit under a Humanoid that has an Animator");
            }
            continue;
        };
        let Some(runtime) = index.by_animator.get(&animator).and_then(|e| runtimes.get(*e).ok()) else { continue };
        let Some(view) = runtime.skeleton() else { continue };
        let Ok((avatar_global, avatar_tf, body, intent, script, rig, calibrating)) = avatars.get(view.avatar) else {
            continue;
        };
        // The feet are calibrated to the ground a while after the pose settles.
        if calibrating {
            continue;
        }
        let to_avatar = avatar_global.affine().inverse();

        let rebuild = shadow.built.as_ref().is_none_or(|b| b.settings != *database || b.avatar != view.avatar);
        if rebuild {
            if shadow.retry_in > 0 {
                shadow.retry_in -= 1;
                continue;
            }
            let built = (|| -> Result<Built, (bool, String)> {
                let wait = |text: String| (true, text);
                let fail = |text: String| (false, text);
                let skeleton_root = view.skeleton_root.ok_or_else(|| fail("the rig has no skeleton root".into()))?;
                let root_global = globals.get(skeleton_root).map_err(|_| wait("the skeleton root is not placed yet".into()))?;
                let root = Transform::from_matrix(Mat4::from(to_avatar) * Mat4::from(root_global.affine()));
                let joints: Vec<(Entity, AnimationTargetId, Transform)> =
                    view.joints.joints.iter().map(|j| (j.entity, j.target, j.rest)).collect();
                let bone = |b: HumanoidBone, name: &str| rig.bone(b).ok_or_else(|| fail(format!("the rig has no {name}")));
                let skeleton = build_skeleton(
                    &joints,
                    &|e| parents.get(e).ok().map(|p| p.parent()),
                    bone(HumanoidBone::Hips, "hips")?,
                    bone(HumanoidBone::LeftFoot, "left foot")?,
                    bone(HumanoidBone::RightFoot, "right foot")?,
                    root,
                )
                .map_err(fail)?;
                let mut clips: HashMap<String, &AnimationClip> = HashMap::new();
                for setting in &database.clips {
                    let handle = runtime
                        .ready_clip(&setting.content)
                        .ok_or_else(|| wait(format!("{:?} ({}) is not loaded yet", setting.name, setting.content)))?;
                    let clip = clip_assets.get(handle).ok_or_else(|| wait(format!("{:?} is not an asset yet", setting.name)))?;
                    clips.insert(setting.content.clone(), clip);
                }
                let reference = reference_from_rest(&skeleton.shape, skeleton.landmarks, Quat::IDENTITY).map_err(fail)?;
                let db = build_database(database, &skeleton, &reference, &clips).map_err(fail)?;
                Ok(Built { db, skeleton, reference, settings: database.clone(), avatar: view.avatar })
            })();
            match built {
                Ok(built) => {
                    if let Some(database_id) = job.settings.database {
                        sizes.push((database_id, built.db.clip_count(), built.db.frame_count()));
                    }
                    info!(
                        "motion matcher (shadow): a database of {} clips, {} frames, for {:?}",
                        built.db.clip_count(),
                        built.db.frame_count(),
                        job.id
                    );
                    shadow.built = Some(built);
                    shadow.matcher.reset();
                    shadow.previous = None;
                }
                Err((true, why)) => {
                    if shadow.first(&why) {
                        info!("motion matcher: waiting, {why}");
                    }
                    continue;
                }
                Err((false, why)) => {
                    warn!("motion matcher: cannot build its database: {why}");
                    shadow.retry_in = RETRY_FRAMES;
                    continue;
                }
            }
        }

        let Shadow { built, matcher, previous, since_readout, clip, .. } = shadow;
        let Some(built) = built.as_ref() else { continue };
        let measure = |entity: Entity| {
            globals.get(entity).ok().map(|g| built.reference.measure(to_avatar.transform_point3(g.translation())))
        };
        let landmarks = built.skeleton.landmarks;
        let entities = &built.skeleton.entities;
        let (Some(left), Some(right), Some(hips)) =
            (measure(entities[landmarks.left_foot]), measure(entities[landmarks.right_foot]), measure(entities[landmarks.hips]))
        else {
            continue;
        };
        let measured = Measured { left_foot: left, right_foot: right, hips_height: hips.y };
        let position = avatar_tf.translation;
        let yaw = avatar_tf.rotation.to_euler(EulerRot::YXZ).0;
        let planar = planar_state(position, yaw, previous.as_ref(), dt);
        let controls = intent.map(|i| Controls {
            direction: i.direction,
            sprint: i.sprint,
            walk_speed: body.motion.walk_speed,
            sprint_speed: body.motion.capped_run_and_sprint().1,
            script_yaw: script.filter(|s| !s.auto_rotate).and_then(|s| s.facing_yaw),
        });
        let wish = desired(controls.as_ref(), &planar);
        let mut path = Vec::new();
        predict_trajectory(&planar, &wish, job.settings.responsiveness, built.db.times(), &mut path);
        let query = live_query(path, &measured, previous.as_ref(), &planar, dt);
        *previous = Some(Previous {
            position,
            yaw,
            left_foot: measured.left_foot,
            right_foot: measured.right_foot,
            hips_height: measured.hips_height,
        });

        let Some(step) = matcher.step(&built.db, dt, &query, TagFilter::default(), false) else { continue };
        report(job, &built.db, &step, clip, since_readout, dt, &mut readouts);
    }

    if !readouts.is_empty() || !sizes.is_empty() {
        let mut g = tree.dm.lock();
        for (id, clips, frames) in sizes {
            g.set_prop_from_engine(id, "ClipCount", DmValue::Number(clips as f64));
            g.set_prop_from_engine(id, "FrameCount", DmValue::Number(frames as f64));
        }
        for r in readouts {
            g.set_prop_from_engine(r.id, "CurrentClip", DmValue::String(r.clip));
            g.set_prop_from_engine(r.id, "CurrentTime", DmValue::Number(f64::from(r.time)));
            g.set_prop_from_engine(r.id, "LastCost", DmValue::Number(f64::from(r.cost)));
        }
    }
}

/// Say what the matcher picked: in the Output when `Debug` is on, and into the
/// read-only properties, which are written at most every [`READOUT_EVERY`].
fn report(
    job: &Job,
    db: &Database,
    step: &MatchStep,
    clip: &mut Option<usize>,
    since_readout: &mut f32,
    dt: f32,
    readouts: &mut Vec<Readout>,
) {
    let name = db.clips()[step.clip].name.clone();
    let changed = *clip != Some(step.clip);
    if step.switched && job.settings.debug {
        let before = clip.map_or("nothing".to_string(), |c| db.clips()[c].name.clone());
        info!("motion matcher (shadow): {before} -> {name} at {:.2} s, cost {:.3}", step.time, step.cost);
    }
    *clip = Some(step.clip);
    *since_readout += dt;
    if changed || *since_readout >= READOUT_EVERY {
        *since_readout = 0.0;
        readouts.push(Readout { id: job.id, clip: name, time: step.time, cost: step.cost });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::clip::skeleton_target;
    use bevy::animation::animated_field;
    use bevy::animation::animation_curves::{AnimatableCurve, AnimatedField};
    use bevy::math::curve::UnevenSampleAutoCurve;
    use std::f32::consts::FRAC_PI_2;

    fn entities(n: usize) -> Vec<Entity> {
        let mut world = World::new();
        (0..n).map(|_| world.spawn_empty().id()).collect()
    }

    // ── skeleton ──

    /// Joints given out of order, with a non-joint between a bone and its parent.
    #[test]
    fn a_skeleton_is_ordered_parents_first_through_the_nodes_that_are_not_joints() {
        // hips > (a node that is not a joint) > spine; hips > left; hips > right.
        let e = entities(5);
        let (hips, node, spine, left, right) = (e[0], e[1], e[2], e[3], e[4]);
        let parent: HashMap<Entity, Entity> = [(node, hips), (spine, node), (left, hips), (right, hips)].into_iter().collect();
        let rest = Transform::IDENTITY;
        let joints = vec![
            (spine, skeleton_target("spine"), rest),
            (left, skeleton_target("left"), rest),
            (hips, skeleton_target("hips"), rest),
            (right, skeleton_target("right"), rest),
        ];
        let skeleton =
            build_skeleton(&joints, &|x| parent.get(&x).copied(), hips, left, right, Transform::IDENTITY).unwrap();
        assert_eq!(skeleton.entities[0], hips, "the root comes first");
        let at = |x: Entity| skeleton.entities.iter().position(|e| *e == x).unwrap();
        assert_eq!(skeleton.shape.joints[at(spine)].parent, Some(at(hips)), "the nearest joint above the spine");
        assert_eq!(skeleton.shape.joints[at(left)].parent, Some(at(hips)));
        assert_eq!(skeleton.shape.joints[at(hips)].parent, None);
        assert_eq!(skeleton.landmarks, Landmarks { hips: at(hips), left_foot: at(left), right_foot: at(right) });
        assert!(skeleton.shape.validate().is_ok());
    }

    #[test]
    fn a_landmark_that_is_not_a_joint_is_refused() {
        let e = entities(3);
        let joints = vec![(e[0], skeleton_target("hips"), Transform::IDENTITY)];
        let none = |_: Entity| None;
        assert!(build_skeleton(&joints, &none, e[0], e[1], e[0], Transform::IDENTITY).is_err());
    }

    // ── the character's numbers ──

    #[test]
    fn the_planar_state_is_the_change_since_last_frame() {
        let previous = Previous {
            position: Vec3::new(1.0, 5.0, 2.0),
            yaw: 0.0,
            left_foot: Vec3::ZERO,
            right_foot: Vec3::ZERO,
            hips_height: 1.0,
        };
        let state = planar_state(Vec3::new(1.0, 5.0, 1.9), 0.1, Some(&previous), 0.05);
        assert!((state.velocity - Vec2::new(0.0, -2.0)).length() < 1.0e-4, "2 m/s forward: {:?}", state.velocity);
        assert!((state.yaw_rate - 2.0).abs() < 1.0e-4);
        assert_eq!(state.acceleration, Vec2::ZERO);
        let first = planar_state(Vec3::new(3.0, 0.0, 4.0), 0.5, None, 0.05);
        assert_eq!(first.velocity, Vec2::ZERO, "nothing to compare with on the first frame");
        assert_eq!(first.position, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn a_turn_across_the_wrap_is_the_short_way_in_the_yaw_rate() {
        let previous = Previous { position: Vec3::ZERO, yaw: 3.1, left_foot: Vec3::ZERO, right_foot: Vec3::ZERO, hips_height: 1.0 };
        let state = planar_state(Vec3::ZERO, -3.1, Some(&previous), 0.1);
        assert!(state.yaw_rate > 0.0 && state.yaw_rate < 1.0, "0.083 rad over 0.1 s, not 62 rad/s: {}", state.yaw_rate);
    }

    #[test]
    fn the_controls_set_the_wish() {
        let state = PlanarState { yaw: 0.3, ..Default::default() };
        let controls = Controls { direction: Vec3::new(0.0, 0.0, -1.0), sprint: false, walk_speed: 1.5, sprint_speed: 6.0, script_yaw: None };
        let walking = desired(Some(&controls), &state);
        assert!((walking.velocity - Vec2::new(0.0, -1.5)).length() < 1.0e-5, "north at the walk");
        assert!(walking.yaw.abs() < 1.0e-5, "it faces the way it goes: yaw 0 is -z");
        let sprinting = desired(Some(&Controls { sprint: true, ..controls }), &state);
        assert!((sprinting.velocity.length() - 6.0).abs() < 1.0e-5);
        let still = desired(Some(&Controls { direction: Vec3::ZERO, ..controls }), &state);
        assert_eq!(still.velocity, Vec2::ZERO);
        assert_eq!(still.yaw, 0.3, "standing still keeps its facing");
        let owned = desired(Some(&Controls { script_yaw: Some(1.2), ..controls }), &state);
        assert_eq!(owned.yaw, 1.2, "a script that owns the facing has it");
        // Going east: yaw -90 degrees faces +x.
        let east = desired(Some(&Controls { direction: Vec3::X, ..controls }), &state);
        assert!((east.yaw + FRAC_PI_2).abs() < 1.0e-5, "{}", east.yaw);
    }

    #[test]
    fn a_character_without_controls_wants_what_it_is_doing() {
        let state = PlanarState { velocity: Vec2::new(1.0, 2.0), yaw: 0.7, ..Default::default() };
        assert_eq!(desired(None, &state), Desired { velocity: state.velocity, yaw: 0.7 });
    }

    #[test]
    fn a_live_query_carries_the_feet_and_the_speeds_in_the_characters_frame() {
        let measured = Measured { left_foot: Vec3::new(-0.1, 0.0, 0.5), right_foot: Vec3::new(0.1, 0.2, -0.5), hips_height: 1.1 };
        let previous = Previous {
            position: Vec3::ZERO,
            yaw: 0.0,
            left_foot: Vec3::new(-0.1, 0.0, 0.4),
            right_foot: Vec3::new(0.1, 0.1, -0.4),
            hips_height: 1.0,
        };
        // Facing +x (yaw -90 degrees) and moving along it at 2 m/s.
        let state = PlanarState { velocity: Vec2::new(2.0, 0.0), yaw: -FRAC_PI_2, ..Default::default() };
        let query = live_query(vec![TrajectoryPoint::default()], &measured, Some(&previous), &state, 0.1);
        assert!((query.left_foot_velocity - Vec3::new(0.0, 0.0, 1.0)).length() < 1.0e-4, "{:?}", query.left_foot_velocity);
        assert!((query.right_foot_velocity - Vec3::new(0.0, 1.0, -1.0)).length() < 1.0e-4);
        assert!(query.root_velocity.x.abs() < 1.0e-4 && (query.root_velocity.z + 2.0).abs() < 1.0e-4, "forward is -z: {:?}", query.root_velocity);
        assert!((query.root_velocity.y - 1.0).abs() < 1.0e-4, "the hips rose 0.1 m in 0.1 s");
        let first = live_query(Vec::new(), &measured, None, &state, 0.1);
        assert_eq!(first.left_foot_velocity, Vec3::ZERO);
    }

    // ── settings ──

    fn tree_with_database() -> (DataModel, InstanceId, InstanceId) {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let db = g.create_virtual("MotionDatabase", "Locomotion", Some(ws));
        for (name, id, speed, heading, loops) in [("walk", "rig://walk", 1.45, 0.0, true), ("jump", "rig://jump", 0.0, 0.0, false)] {
            let clip = g.create_virtual("Animation", name, Some(db));
            g.set_prop_from_engine(clip, "AnimationId", DmValue::String(id.into()));
            g.seed_attribute(clip, "Speed", DmValue::Number(speed));
            g.seed_attribute(clip, "Heading", DmValue::Number(heading));
            g.seed_attribute(clip, "Loop", DmValue::Bool(loops));
            g.seed_attribute(clip, "Tags", DmValue::String("locomotion, ground".into()));
        }
        let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(ws));
        let matcher = g.create_virtual("MotionMatcher", "Matcher", Some(humanoid));
        g.set_prop_from_engine(matcher, "Database", DmValue::Instance(db));
        (g, db, matcher)
    }

    #[test]
    fn a_new_matcher_is_off_and_a_database_reads_its_clips() {
        let (mut g, db, matcher) = tree_with_database();
        let settings = MatcherSettings::read(&g, matcher);
        assert_eq!(settings.mode, MatcherMode::Off, "off until a Space turns it on");
        assert_eq!(settings.database, Some(db));
        assert_eq!(settings.search, SearchConfig::default());
        assert!((settings.responsiveness - 0.15).abs() < 1.0e-6 && !settings.debug);
        g.set_prop_from_engine(matcher, "Mode", DmValue::String("Shadow".into()));
        assert_eq!(MatcherSettings::read(&g, matcher).mode, MatcherMode::Shadow);

        let database = DatabaseSettings::read(&g, db);
        assert_eq!(database.times, DEFAULT_TRAJECTORY_TIMES.to_vec());
        assert_eq!(database.clips.len(), 2);
        assert_eq!(database.clips[0].name, "walk");
        assert_eq!(database.clips[0].content, "rig://walk");
        assert_eq!(database.clips[0].spec.speed, 1.45);
        assert!(database.clips[0].spec.looping && !database.clips[1].spec.looping);
        assert_eq!(database.clips[0].spec.tags, vec!["locomotion".to_string(), "ground".to_string()]);
        assert_eq!(database.clips[0].spec.fps, 30.0);
    }

    #[test]
    fn a_property_that_makes_no_sense_falls_back() {
        let (mut g, db, matcher) = tree_with_database();
        g.set_prop_from_engine(db, "TrajectoryTimes", DmValue::String("0.5, 0.2".into()));
        assert_eq!(DatabaseSettings::read(&g, db).times, DEFAULT_TRAJECTORY_TIMES.to_vec(), "descending times");
        g.set_prop_from_engine(db, "TrajectoryTimes", DmValue::String("0.1, 0.3".into()));
        assert_eq!(DatabaseSettings::read(&g, db).times, vec![0.1, 0.3]);
        g.set_prop_from_engine(db, "SampleRate", DmValue::Number(10_000.0));
        assert_eq!(DatabaseSettings::read(&g, db).clips[0].spec.fps, 120.0, "a frame rate is held to a sane range");
        g.set_prop_from_engine(matcher, "Hysteresis", DmValue::Number(7.0));
        assert_eq!(MatcherSettings::read(&g, matcher).search.hysteresis, 1.0);
        // A Database that is not a MotionDatabase is no database.
        let workspace = g.get_service("Workspace");
        let part = g.create_virtual("Folder", "NotOne", workspace);
        g.set_prop_from_engine(matcher, "Database", DmValue::Instance(part));
        assert_eq!(MatcherSettings::read(&g, matcher).database, None);
    }

    #[test]
    fn the_modes_parse_by_name() {
        assert_eq!(MatcherMode::parse("Shadow"), MatcherMode::Shadow);
        assert_eq!(MatcherMode::parse(" drive "), MatcherMode::Drive);
        assert_eq!(MatcherMode::parse("anything else"), MatcherMode::Off);
        assert_eq!(parse_times("0.2,0.4,0.6,1.0"), Some(vec![0.2, 0.4, 0.6, 1.0]));
        assert_eq!(parse_times(""), None);
        assert_eq!(parse_times("0.2,x"), None);
        assert_eq!(parse_times("0,1"), None, "a time of zero is not ahead");
    }

    #[test]
    fn only_matchers_that_are_on_are_jobs() {
        let (mut g, _db, matcher) = tree_with_database();
        assert!(jobs(&g).is_empty(), "off");
        g.set_prop_from_engine(matcher, "Mode", DmValue::String("Shadow".into()));
        let all = jobs(&g);
        assert_eq!(all.len(), 1);
        assert!(all[0].database.is_some());
        assert_eq!(all[0].animator, None, "the Humanoid has no Animator here");
        let humanoid = g.parent(matcher).unwrap();
        let animator = g.create_virtual("Animator", "Animator", Some(humanoid));
        assert_eq!(jobs(&g)[0].animator, Some(animator));
    }

    // ── end to end, without a scene ──

    /// A skeleton: hips 1 m up, a foot under each hip.
    fn walker() -> (LiveSkeleton, Reference) {
        let e = entities(3);
        let rest = |x: f32, y: f32| Transform::from_translation(Vec3::new(x, y, 0.0));
        let joints = vec![
            (e[0], skeleton_target("hips"), rest(0.0, 1.0)),
            (e[1], skeleton_target("left"), rest(-0.1, -1.0)),
            (e[2], skeleton_target("right"), rest(0.1, -1.0)),
        ];
        let parent: HashMap<Entity, Entity> = [(e[1], e[0]), (e[2], e[0])].into_iter().collect();
        let skeleton =
            build_skeleton(&joints, &|x| parent.get(&x).copied(), e[0], e[1], e[2], Transform::IDENTITY).unwrap();
        let reference = reference_from_rest(&skeleton.shape, skeleton.landmarks, Quat::IDENTITY).unwrap();
        (skeleton, reference)
    }

    /// A clip in which each foot swings along z, in turn, once a second.
    fn striding() -> AnimationClip {
        let mut clip = AnimationClip::default();
        for (key, x, from) in [("left", -0.1, -0.35), ("right", 0.1, 0.35)] {
            let curve = UnevenSampleAutoCurve::new([
                (0.0, Vec3::new(x, -1.0, from)),
                (0.5, Vec3::new(x, -1.0, -from)),
                (1.0, Vec3::new(x, -1.0, from)),
            ])
            .unwrap();
            clip.add_curve_to_target(skeleton_target(key), AnimatableCurve::new(animated_field!(Transform::translation), curve));
        }
        clip
    }

    /// A clip that holds still.
    fn standing() -> AnimationClip {
        let mut clip = AnimationClip::default();
        for (key, x) in [("left", -0.1), ("right", 0.1)] {
            let curve = UnevenSampleAutoCurve::new([(0.0, Vec3::new(x, -1.0, 0.0)), (1.0, Vec3::new(x, -1.0, 0.0))]).unwrap();
            clip.add_curve_to_target(skeleton_target(key), AnimatableCurve::new(animated_field!(Transform::translation), curve));
        }
        clip
    }

    fn settings() -> DatabaseSettings {
        let spec = |name: &str, speed: f32| ClipSpec { name: name.into(), fps: 30.0, speed, ..Default::default() };
        DatabaseSettings {
            times: DEFAULT_TRAJECTORY_TIMES.to_vec(),
            weights: FeatureWeights::default(),
            clips: vec![
                ClipSetting { name: "walk".into(), content: "rig://walk".into(), spec: spec("walk", 1.45) },
                ClipSetting { name: "idle".into(), content: "rig://idle".into(), spec: spec("idle", 0.0) },
            ],
        }
    }

    #[test]
    fn a_database_builds_from_the_loaded_clips_and_says_which_is_missing() {
        let (skeleton, reference) = walker();
        let (walk, idle) = (striding(), standing());
        let mut clips: HashMap<String, &AnimationClip> = HashMap::new();
        clips.insert("rig://walk".into(), &walk);
        let missing = build_database(&settings(), &skeleton, &reference, &clips);
        assert!(missing.unwrap_err().contains("rig://idle"), "names the clip that is not loaded");
        clips.insert("rig://idle".into(), &idle);
        let db = build_database(&settings(), &skeleton, &reference, &clips).unwrap();
        assert_eq!(db.clip_count(), 2);
        assert_eq!(db.frame_count(), 60);
    }

    /// A character walking forward gets the walk, and one standing gets the idle.
    #[test]
    fn the_matcher_tells_a_walk_from_standing_with_nothing_but_the_numbers() {
        let (skeleton, reference) = walker();
        let (walk, idle) = (striding(), standing());
        let mut clips: HashMap<String, &AnimationClip> = HashMap::new();
        clips.insert("rig://walk".into(), &walk);
        clips.insert("rig://idle".into(), &idle);
        let db = build_database(&settings(), &skeleton, &reference, &clips).unwrap();
        let walk_index = db.clips().iter().position(|c| c.name == "walk").unwrap();
        let idle_index = db.clips().iter().position(|c| c.name == "idle").unwrap();

        // A character facing -z, moving forward at the walk, left foot back,
        // as the walk clip has it at time 0.
        let measure = |p: Vec3| reference.measure(p);
        let dt = 1.0 / 30.0;
        let controls = Controls { direction: Vec3::NEG_Z, sprint: false, walk_speed: 1.45, sprint_speed: 3.9, script_yaw: None };
        let drive = |controls: &Controls, velocity: Vec2, feet: (Vec3, Vec3), previous: Previous| {
            let state = PlanarState { velocity, yaw: 0.0, ..Default::default() };
            let wish = desired(Some(controls), &state);
            let mut path = Vec::new();
            predict_trajectory(&state, &wish, 0.15, db.times(), &mut path);
            let measured = Measured { left_foot: measure(feet.0), right_foot: measure(feet.1), hips_height: measure(Vec3::new(0.0, 1.0, 0.0)).y };
            live_query(path, &measured, Some(&previous), &state, dt)
        };
        // Foot positions of the walk clip at 0 s and one frame later.
        let (l0, r0) = (Vec3::new(-0.1, 0.0, -0.35), Vec3::new(0.1, 0.0, 0.35));
        let step = 2.0 * 0.7 / 1.0 * dt;
        let (l1, r1) = (Vec3::new(-0.1, 0.0, -0.35 + step), Vec3::new(0.1, 0.0, 0.35 - step));
        let previous = Previous { position: Vec3::ZERO, yaw: 0.0, left_foot: measure(l0), right_foot: measure(r0), hips_height: 1.0 };
        let walking = drive(&controls, Vec2::new(0.0, -1.45), (l1, r1), previous);
        let mut matcher = Matcher::new(SearchConfig::default());
        let picked = matcher.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        assert_eq!(picked.clip, walk_index, "walking forward matches the walk");

        let rest_feet = (Vec3::new(-0.1, 0.0, 0.0), Vec3::new(0.1, 0.0, 0.0));
        let still = Controls { direction: Vec3::ZERO, ..controls };
        let at_rest = Previous { left_foot: measure(rest_feet.0), right_foot: measure(rest_feet.1), ..previous };
        let standing_query = drive(&still, Vec2::ZERO, rest_feet, at_rest);
        let mut matcher = Matcher::new(SearchConfig::default());
        assert_eq!(matcher.step(&db, 0.0, &standing_query, TagFilter::default(), false).unwrap().clip, idle_index);
    }

    #[test]
    fn the_phase_of_an_instance_is_a_fraction() {
        let (_, _, matcher) = tree_with_database();
        let phase = phase_of(matcher);
        assert!((0.0..1.0).contains(&phase));
        assert_eq!(phase, phase_of(matcher), "stable for one instance");
    }
}

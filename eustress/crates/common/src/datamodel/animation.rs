//! # Animation tracks in the tree
//!
//! `Animator:LoadAnimation` makes an `AnimationTrack` instance, and this module
//! keeps each track's control state and advances it on the session clock
//! (`frame.time`). A track is a pure function of its control changes and the
//! clock, so two trees that saw the same changes hold the same `TimePosition`
//! and `WeightCurrent` at the same time: replication sends control changes,
//! never poses, and a late joiner lands mid-cycle exactly.
//!
//! Both script languages call the methods here. The pose evaluator
//! (`crate::animation`) reads [`DataModel::animation_frame`] every frame and
//! reports each clip's length, keyframes and markers back through
//! [`DataModel::set_clip_info`] once the clip is loaded.
//!
//! The semantics are Roblox's; `docs/design/ANIMATION_SYSTEM.md` lists them
//! and the few places Eustress differs on purpose.
//!
//! ## Frame order
//!
//! [`DataModel::step_animation`] runs once a frame, after the clock advances
//! and before scripts, so the events it raises (`Stopped` at a clip's end,
//! `Ended`, `DidLoop`, keyframes and markers) reach scripts in the same frame
//! on both shells. The Player trims every event at the end of its apply step,
//! so an event raised after its scripts ran would never be read there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{DataModel, DmEvent, DmValue, EnumItem, InstanceId, OutputLevel};

/// Roblox's limit of loaded tracks per Animator.
pub const MAX_TRACKS_PER_ANIMATOR: usize = 256;

/// Roblox's default fade time for `Play`, `Stop` and `AdjustWeight`.
pub const DEFAULT_FADE: f32 = 0.1;

/// Effective weights below this are exactly zero. Bevy skips a clip whose
/// weight is exactly zero before it contributes, which is what keeps two
/// silent contributors from blending to 0/0.
pub const WEIGHT_EPSILON: f32 = 1e-6;

/// `DidLoop` events one track raises in one step. A very short clip played
/// fast would otherwise flood the queue after a long frame.
const MAX_LOOPS_PER_STEP: i64 = 8;

/// Keyframe and marker events one track raises in one step.
const MAX_CROSSINGS_PER_STEP: usize = 64;

/// The longest `AnimationId` a track keeps.
const MAX_CONTENT_LEN: usize = 1024;

/// Speeds beyond this are clamped: a track cannot outrun the clock that much.
const MAX_SPEED: f32 = 100.0;

/// The name `KeyframeReached` ignores, as Roblox does.
const DEFAULT_KEYFRAME_NAME: &str = "Keyframe";

// ============================================================================
// Priority
// ============================================================================

/// Roblox's `Enum.AnimationPriority`, ordered lowest to highest. `Core` is the
/// lowest although its enum value is 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    #[default]
    Core,
    Idle,
    Movement,
    Action,
    Action2,
    Action3,
    Action4,
}

impl Priority {
    /// Every priority, highest first: the order the blending rule ranks in.
    pub const HIGHEST_FIRST: [Priority; 7] = [
        Priority::Action4,
        Priority::Action3,
        Priority::Action2,
        Priority::Action,
        Priority::Movement,
        Priority::Idle,
        Priority::Core,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Priority::Core => "Core",
            Priority::Idle => "Idle",
            Priority::Movement => "Movement",
            Priority::Action => "Action",
            Priority::Action2 => "Action2",
            Priority::Action3 => "Action3",
            Priority::Action4 => "Action4",
        }
    }

    /// `"Action"`, `"Enum.AnimationPriority.Action"` or `"AnimationPriority.Action"`.
    pub fn from_name(name: &str) -> Option<Self> {
        let n = name.trim();
        let n = n.strip_prefix("Enum.").unwrap_or(n);
        let n = n.strip_prefix("AnimationPriority.").unwrap_or(n);
        Priority::HIGHEST_FIRST.into_iter().find(|p| p.name().eq_ignore_ascii_case(n))
    }

    /// Roblox's numeric value, as rbxm files store it.
    pub fn roblox_value(self) -> u32 {
        match self {
            Priority::Idle => 0,
            Priority::Movement => 1,
            Priority::Action => 2,
            Priority::Action2 => 3,
            Priority::Action3 => 4,
            Priority::Action4 => 5,
            Priority::Core => 1000,
        }
    }

    pub fn from_roblox_value(value: u32) -> Option<Self> {
        Priority::HIGHEST_FIRST.into_iter().find(|p| p.roblox_value() == value)
    }

    pub fn enum_item(self) -> EnumItem {
        EnumItem::new("AnimationPriority", self.name())
    }

    /// The priority a script wrote: the enum item, its name, or its number.
    pub fn from_value(value: &DmValue) -> Option<Self> {
        match value {
            DmValue::Enum(e) => Priority::from_name(&e.name),
            DmValue::String(s) => Priority::from_name(s),
            DmValue::Number(n) if n.is_finite() && *n >= 0.0 => Priority::from_roblox_value(*n as u32),
            _ => None,
        }
    }
}

// ============================================================================
// Control state
// ============================================================================

/// Everything a track's playback depends on. Its time and weight at any clock
/// time follow from this alone, so replication sends exactly this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackControl {
    /// `IsPlaying`: true from `Play` until `Stop` or the end of a non-looped clip.
    pub playing: bool,
    pub looped: bool,
    pub priority: Priority,
    /// The clock time the time line was anchored at.
    pub anchor: f64,
    /// The track time at `anchor`.
    pub anchor_time: f32,
    /// Seconds of clip per second of clock: negative plays backward, 0 pauses.
    pub speed: f32,
    /// True once a non-looped clip has reached its end: the time holds there.
    pub held: bool,
    /// `WeightCurrent` moves from `fade_from` to `fade_to` over `fade_secs`,
    /// starting at `fade_start`.
    pub fade_start: f64,
    pub fade_from: f32,
    pub fade_to: f32,
    pub fade_secs: f32,
    /// The fade time of the last `Play`, used when a non-looped clip ends.
    pub end_fade: f32,
    /// Counts of `Play` and `Stop` calls. A machine applying a replicated
    /// control state raises `AnimationPlayed` and `Stopped` when they change.
    pub plays: u32,
    pub stops: u32,
}

impl Default for TrackControl {
    fn default() -> Self {
        Self {
            playing: false,
            looped: false,
            priority: Priority::Core,
            anchor: 0.0,
            anchor_time: 0.0,
            speed: 1.0,
            held: false,
            fade_start: 0.0,
            fade_from: 0.0,
            fade_to: 0.0,
            fade_secs: 0.0,
            end_fade: DEFAULT_FADE,
            plays: 0,
            stops: 0,
        }
    }
}

impl TrackControl {
    /// `WeightCurrent` at clock time `now`.
    pub fn weight_at(&self, now: f64) -> f32 {
        if !(self.fade_secs > 0.0) {
            return self.fade_to;
        }
        let t = ((now - self.fade_start) / self.fade_secs as f64).clamp(0.0, 1.0) as f32;
        self.fade_from + (self.fade_to - self.fade_from) * t
    }

    /// The unwrapped clip time at `now`: anchored time plus elapsed clock
    /// times speed. A held track stays where it stopped.
    pub fn raw_time_at(&self, now: f64) -> f64 {
        if self.held {
            return self.anchor_time as f64;
        }
        self.anchor_time as f64 + (now - self.anchor) * self.speed as f64
    }

    /// `TimePosition` at `now` for a clip of `length` seconds (0 when the clip
    /// has not loaded yet).
    pub fn time_at(&self, now: f64, length: f32) -> f32 {
        clip_time(self.raw_time_at(now), length, self.looped).0
    }
}

/// The clip time an unwrapped time shows, and how many whole cycles lie
/// before it. Before a clip loads (`length` 0) the time runs on unbounded.
pub fn clip_time(raw: f64, length: f32, looped: bool) -> (f32, i64) {
    if !(length > 0.0) {
        return (raw.max(0.0) as f32, 0);
    }
    let len = length as f64;
    if looped {
        let cycles = (raw / len).floor();
        let t = raw - cycles * len;
        (t.clamp(0.0, len) as f32, cycles as i64)
    } else {
        (raw.clamp(0.0, len) as f32, 0)
    }
}

// ============================================================================
// The blending rule
// ============================================================================

/// Roblox's blending rule for one joint. `tracks` holds the priority and
/// `WeightCurrent` of each track that animates the joint.
///
/// Tracks are ranked from the highest priority to the lowest; a priority takes
/// what is left of a total weight of 1; tracks of one priority share their part
/// in proportion to their weights; the rest pose takes whatever remains.
/// Returns each track's effective weight, in `tracks`' order, and the rest
/// pose's. Weights under [`WEIGHT_EPSILON`] come back as exactly zero.
pub fn effective_weights(tracks: &[(Priority, f32)]) -> (Vec<f32>, f32) {
    let mut out = vec![0.0_f32; tracks.len()];
    let mut remaining = 1.0_f32;
    for level in Priority::HIGHEST_FIRST {
        if remaining <= WEIGHT_EPSILON {
            break;
        }
        let total: f32 = tracks
            .iter()
            .filter(|(p, w)| *p == level && w.is_finite() && *w > 0.0)
            .map(|(_, w)| *w)
            .sum();
        if total <= WEIGHT_EPSILON {
            continue;
        }
        let take = total.min(remaining);
        for (i, (p, w)) in tracks.iter().enumerate() {
            if *p == level && w.is_finite() && *w > 0.0 {
                out[i] = w / total * take;
            }
        }
        remaining -= take;
    }
    for w in out.iter_mut() {
        if *w < WEIGHT_EPSILON {
            *w = 0.0;
        }
    }
    let rest = if remaining < WEIGHT_EPSILON { 0.0 } else { remaining };
    (out, rest)
}

// ============================================================================
// Tracks, clips and ops
// ============================================================================

/// What the evaluator learned about a clip once it loaded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipInfo {
    /// Seconds.
    pub length: f32,
    /// The clip's own `Loop`, which a new track starts with.
    pub looped: bool,
    /// The clip's own `Priority`, which a new track starts with.
    pub priority: Priority,
    /// Every keyframe's time and name, in time order.
    pub keyframes: Vec<(f32, String)>,
    /// Every marker's time, name and value, in time order.
    pub markers: Vec<(f32, String, String)>,
}

/// One loaded track.
#[derive(Debug, Clone)]
pub struct Track {
    pub animator: InstanceId,
    pub animation: Option<InstanceId>,
    /// The Animation's `AnimationId` when the track was loaded.
    pub content: String,
    pub control: TrackControl,
    /// The script set `Looped` or `Priority`, so the clip's own values do not
    /// replace them when it finishes loading.
    looped_set: bool,
    priority_set: bool,
    /// The clip's facts were applied to this track.
    clip_applied: bool,
    /// The unwrapped time at the last step; events lie between it and now.
    last_raw: f64,
    /// Set by `Play`: the next step also counts the instant playback started.
    include_start: bool,
    /// Stopped or ended, and still fading out: `Ended` fires when it reaches 0.
    ending: bool,
    /// Loaded past the per-Animator limit: plays nothing.
    inert: bool,
    /// Loaded by the session for another machine's script: its changes are
    /// never logged, so nothing echoes back.
    remote: bool,
    /// Load order, for evicting the oldest finished track at the limit.
    serial: u64,
}

impl Track {
    pub fn is_inert(&self) -> bool {
        self.inert
    }

    /// The session made this track for another machine's script.
    pub fn is_remote(&self) -> bool {
        self.remote
    }
}

/// A control change for replication. The session drains these with
/// [`DataModel::take_track_ops`] while `networked` is set, keeps the newest
/// `Control` per track, and maps instance ids to its own ids.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackOp {
    /// `Animator:LoadAnimation` made `track`.
    Load { animator: InstanceId, track: InstanceId, animation: Option<InstanceId>, content: String, name: String },
    /// The whole control state after a change. Idempotent: the newest wins.
    Control { track: InstanceId, control: TrackControl },
    /// `Destroy()`, or the Animator went away.
    Unload { track: InstanceId },
}

/// One track as the evaluator sees it this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSample {
    pub track: InstanceId,
    pub content: String,
    /// `TimePosition`, seconds.
    pub time: f32,
    /// `WeightCurrent`.
    pub weight: f32,
    pub priority: Priority,
}

/// One Animator's tracks this frame: every track that is playing or still
/// fading out, in track id order, so membership is stable between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatorFrame {
    pub animator: InstanceId,
    pub tracks: Vec<TrackSample>,
}

/// The tree's animation state. Lives on [`DataModel::animation`].
#[derive(Debug, Default)]
pub struct AnimationState {
    tracks: BTreeMap<InstanceId, Track>,
    by_animator: BTreeMap<InstanceId, Vec<InstanceId>>,
    clips: BTreeMap<(InstanceId, String), ClipInfo>,
    ops: Vec<TrackOp>,
    /// Animators already warned about the track limit.
    warned_full: BTreeSet<InstanceId>,
    next_serial: u64,
    /// `KeyframeSequenceProvider:RegisterKeyframeSequence`: `active://<n>` to
    /// its sequence.
    registered: BTreeMap<u64, InstanceId>,
    next_registered: u64,
    /// The Space folder clip records are read from, kept current by the
    /// evaluator, so `GetKeyframeSequenceAsync` can read a record.
    space_root: Option<PathBuf>,
    /// Each Humanoid's state from frame to frame
    /// (`crate::animation::humanoid::report_state`).
    pub(crate) humanoids: BTreeMap<InstanceId, crate::animation::humanoid::HumanoidStateTracker>,
    /// This frame's track events, for scripts that poll (Rune).
    frame_events: Vec<TrackEvent>,
}

/// A track event, as a script that polls reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEvent {
    pub track: InstanceId,
    /// `stopped`, `ended`, `looped`, `keyframe` or `marker`.
    pub kind: &'static str,
    /// The keyframe's or marker's name.
    pub name: String,
    /// The marker's value.
    pub value: String,
}

impl TrackEvent {
    /// The event a track signal is, if it is one.
    fn of(e: &DmEvent) -> Option<TrackEvent> {
        let DmEvent::Signal { id, name, args } = e else { return None };
        let text = |i: usize| args.get(i).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let (kind, name, value) = match name.as_str() {
            "Stopped" => ("stopped", String::new(), String::new()),
            "Ended" => ("ended", String::new(), String::new()),
            "DidLoop" => ("looped", String::new(), String::new()),
            "KeyframeReached" => ("keyframe", text(0), String::new()),
            other => match other.strip_prefix("Marker:") {
                Some(marker) => ("marker", marker.to_string(), text(0)),
                None => return None,
            },
        };
        Some(TrackEvent { track: *id, kind, name, value })
    }
}

impl AnimationState {
    /// Every loaded track, in id order. Replication reads this for a late
    /// joiner's snapshot.
    pub fn tracks(&self) -> impl Iterator<Item = (InstanceId, &Track)> + '_ {
        self.tracks.iter().map(|(id, t)| (*id, t))
    }

    pub fn track(&self, id: InstanceId) -> Option<&Track> {
        self.tracks.get(&id)
    }

    /// This frame's track events, in the order they fired.
    pub fn frame_events(&self) -> &[TrackEvent] {
        &self.frame_events
    }

    pub fn space_root(&self) -> Option<&Path> {
        self.space_root.as_deref()
    }

    pub fn set_space_root(&mut self, root: Option<PathBuf>) {
        self.space_root = root;
    }

    /// Forget everything: a Play session ended.
    pub fn clear(&mut self) {
        *self = AnimationState::default();
    }
}

/// A signal an animation event fires on an instance.
fn signal(id: InstanceId, name: impl Into<String>, args: Vec<DmValue>) -> DmEvent {
    DmEvent::Signal { id, name: name.into(), args }
}

fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

// ============================================================================
// The DataModel's animation API
// ============================================================================

impl DataModel {
    /// The Animator a `LoadAnimation` on `owner` uses: `owner` itself when it
    /// is an Animator, else its Animator child, made when there is none, as
    /// Roblox's deprecated `Humanoid:LoadAnimation` and
    /// `AnimationController:LoadAnimation` do.
    pub fn animator_of(&mut self, owner: InstanceId) -> Result<InstanceId, String> {
        let class = self.class_of(owner).ok_or("LoadAnimation on a destroyed instance")?.to_string();
        match class.as_str() {
            "Animator" => Ok(owner),
            "Humanoid" | "AnimationController" => {
                if let Some(a) = self.find_first_child_of_class(owner, "Animator", false) {
                    return Ok(a);
                }
                let a = self.create("Animator");
                self.set_parent(a, Some(owner))?;
                Ok(a)
            }
            other => Err(format!("LoadAnimation is not a valid member of {other}")),
        }
    }

    /// The Animator `owner` names without making one: itself, or its child.
    fn existing_animator(&self, owner: InstanceId) -> Option<InstanceId> {
        match self.class_of(owner)? {
            "Animator" => Some(owner),
            "Humanoid" | "AnimationController" => self.find_first_child_of_class(owner, "Animator", false),
            _ => None,
        }
    }

    /// `Animator:LoadAnimation(animation)`, and the Humanoid and
    /// AnimationController proxies.
    pub fn load_animation(&mut self, owner: InstanceId, animation: InstanceId) -> Result<InstanceId, String> {
        match self.class_of(animation) {
            Some("Animation") => {}
            Some(other) => return Err(format!("LoadAnimation expects an Animation, got {other}")),
            None => return Err("LoadAnimation expects an Animation, got a destroyed instance".into()),
        }
        let animator = self.animator_of(owner)?;
        let content = self
            .get_prop(animation, "AnimationId")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let name = self.name_of(animation).unwrap_or("Animation").to_string();
        Ok(self.load_track(animator, Some(animation), &content, &name, true))
    }

    /// A track from a content id alone, with no Animation instance. Rune's
    /// `load_animation_id`.
    pub fn load_animation_content(&mut self, owner: InstanceId, content: &str) -> Result<InstanceId, String> {
        let animator = self.animator_of(owner)?;
        let name = content.rsplit(|c: char| c == '/' || c == ':').next().unwrap_or("Animation").to_string();
        Ok(self.load_track(animator, None, content, &name, true))
    }

    /// A track a replicated `Load` names. Nothing is logged: it came from the
    /// session.
    pub fn load_remote_track(
        &mut self,
        animator: InstanceId,
        animation: Option<InstanceId>,
        content: &str,
        name: &str,
    ) -> InstanceId {
        self.load_track(animator, animation, content, name, false)
    }

    fn load_track(
        &mut self,
        animator: InstanceId,
        animation: Option<InstanceId>,
        content: &str,
        name: &str,
        log: bool,
    ) -> InstanceId {
        let content: String = content.chars().take(MAX_CONTENT_LEN).collect();

        // At the limit, release the oldest track that finished and faded out.
        let mut inert = false;
        let loaded = self.animation.by_animator.get(&animator).map_or(0, Vec::len);
        if loaded >= MAX_TRACKS_PER_ANIMATOR {
            let now = self.frame.time;
            let oldest = self
                .animation
                .by_animator
                .get(&animator)
                .into_iter()
                .flatten()
                .filter_map(|id| self.animation.tracks.get(id).map(|t| (*id, t)))
                .filter(|(_, t)| !t.control.playing && t.control.weight_at(now) <= WEIGHT_EPSILON)
                .min_by_key(|(_, t)| t.serial)
                .map(|(id, _)| id);
            match oldest {
                Some(old) => self.unload_track(old),
                None => {
                    inert = true;
                    if self.animation.warned_full.insert(animator) {
                        self.print(
                            OutputLevel::Warn,
                            "Animator",
                            format!(
                                "AnimationTrack limit of {MAX_TRACKS_PER_ANIMATOR} tracks for one Animator exceeded, \
                                 new animations will not be played"
                            ),
                        );
                    }
                }
            }
        }

        let id = self.create("AnimationTrack");
        let control = TrackControl::default();
        if let Some(inst) = self.get_mut(id) {
            inst.name = name.to_string();
            let props = &mut inst.props;
            props.insert("Animation".into(), animation.map_or(DmValue::Nil, DmValue::Instance));
            props.insert("IsPlaying".into(), DmValue::Bool(false));
            props.insert("Length".into(), DmValue::Number(0.0));
            props.insert("Looped".into(), DmValue::Bool(control.looped));
            props.insert("Priority".into(), DmValue::Enum(control.priority.enum_item()));
            props.insert("Speed".into(), DmValue::Number(control.speed as f64));
            props.insert("TimePosition".into(), DmValue::Number(0.0));
            props.insert("WeightCurrent".into(), DmValue::Number(0.0));
            props.insert("WeightTarget".into(), DmValue::Number(0.0));
        }
        let serial = self.animation.next_serial;
        self.animation.next_serial += 1;
        self.animation.tracks.insert(
            id,
            Track {
                animator,
                animation,
                content: content.clone(),
                control,
                looped_set: false,
                priority_set: false,
                clip_applied: false,
                last_raw: 0.0,
                include_start: false,
                ending: false,
                inert,
                remote: !log,
                serial,
            },
        );
        self.animation.by_animator.entry(animator).or_default().push(id);
        if log && self.networked {
            self.animation.ops.push(TrackOp::Load { animator, track: id, animation, content, name: name.to_string() });
        }
        // A clip this Animator already loaded gives the new track its facts now.
        self.apply_clip_to_track(id);
        id
    }

    pub fn is_track(&self, id: InstanceId) -> bool {
        self.animation.tracks.contains_key(&id)
    }

    /// Every track loaded on `owner`'s Animator.
    pub fn tracks_of(&self, owner: InstanceId) -> Vec<InstanceId> {
        self.existing_animator(owner)
            .and_then(|a| self.animation.by_animator.get(&a).cloned())
            .unwrap_or_default()
    }

    /// `GetPlayingAnimationTracks`: every track playing or still fading out.
    pub fn playing_tracks(&self, owner: InstanceId) -> Vec<InstanceId> {
        let now = self.frame.time;
        self.tracks_of(owner)
            .into_iter()
            .filter(|id| {
                self.animation
                    .tracks
                    .get(id)
                    .is_some_and(|t| t.control.playing || t.control.weight_at(now) > WEIGHT_EPSILON)
            })
            .collect()
    }

    fn clip_length(&self, track: &Track) -> f32 {
        self.animation.clips.get(&(track.animator, track.content.clone())).map_or(0.0, |c| c.length)
    }

    /// `AnimationTrack:Play(fadeTime, weight, speed)`.
    pub fn play_track(&mut self, track: InstanceId, fade: f32, weight: f32, speed: f32) -> Result<(), String> {
        let now = self.frame.time;
        let (animator, events) = {
            let len = {
                let t = self.animation.tracks.get(&track).ok_or("Play on an unloaded AnimationTrack")?;
                self.clip_length(t)
            };
            let t = self.animation.tracks.get_mut(&track).ok_or("Play on an unloaded AnimationTrack")?;
            let fade = finite_or(fade, DEFAULT_FADE).max(0.0);
            let weight = finite_or(weight, 1.0).max(0.0);
            let speed = finite_or(speed, 1.0).clamp(-MAX_SPEED, MAX_SPEED);
            let c = &mut t.control;
            let from = c.weight_at(now);
            c.playing = true;
            c.held = false;
            c.speed = speed;
            c.anchor = now;
            c.anchor_time = if speed < 0.0 && len > 0.0 { len } else { 0.0 };
            c.fade_start = now;
            c.fade_from = from;
            c.fade_to = weight;
            c.fade_secs = fade;
            c.end_fade = fade;
            c.plays = c.plays.wrapping_add(1);
            t.last_raw = c.anchor_time as f64;
            t.include_start = true;
            t.ending = false;
            (t.animator, vec![signal(t.animator, "AnimationPlayed", vec![DmValue::Instance(track)])])
        };
        self.log_control(track);
        for e in events {
            self.push_event(e);
        }
        self.echo_played_on_owner(animator, track);
        self.mirror_track(track);
        Ok(())
    }

    /// Roblox raises `AnimationPlayed` on the Humanoid and the
    /// AnimationController too.
    fn echo_played_on_owner(&mut self, animator: InstanceId, track: InstanceId) {
        if let Some(parent) = self.parent(animator) {
            if matches!(self.class_of(parent), Some("Humanoid" | "AnimationController")) {
                self.push_event(signal(parent, "AnimationPlayed", vec![DmValue::Instance(track)]));
            }
        }
    }

    /// `AnimationTrack:Stop(fadeTime)`.
    pub fn stop_track(&mut self, track: InstanceId, fade: f32) -> Result<(), String> {
        let now = self.frame.time;
        let was_playing = {
            let t = self.animation.tracks.get_mut(&track).ok_or("Stop on an unloaded AnimationTrack")?;
            let c = &mut t.control;
            let w = c.weight_at(now);
            if !c.playing && w <= WEIGHT_EPSILON {
                return Ok(());
            }
            let was_playing = c.playing;
            c.playing = false;
            c.fade_start = now;
            c.fade_from = w;
            c.fade_to = 0.0;
            c.fade_secs = finite_or(fade, DEFAULT_FADE).max(0.0);
            c.stops = c.stops.wrapping_add(1);
            t.ending = true;
            was_playing
        };
        self.log_control(track);
        if was_playing {
            self.push_track_event(signal(track, "Stopped", Vec::new()));
        }
        self.mirror_track(track);
        Ok(())
    }

    /// `AnimationTrack:AdjustSpeed(speed)`.
    pub fn adjust_track_speed(&mut self, track: InstanceId, speed: f32) -> Result<(), String> {
        let now = self.frame.time;
        {
            let len = {
                let t = self.animation.tracks.get(&track).ok_or("AdjustSpeed on an unloaded AnimationTrack")?;
                self.clip_length(t)
            };
            let t = self.animation.tracks.get_mut(&track).ok_or("AdjustSpeed on an unloaded AnimationTrack")?;
            let c = &mut t.control;
            let at = clip_time(c.raw_time_at(now), len, c.looped).0;
            c.anchor = now;
            c.anchor_time = at;
            c.speed = finite_or(speed, 1.0).clamp(-MAX_SPEED, MAX_SPEED);
            t.last_raw = at as f64;
            t.include_start = false;
        }
        self.log_control(track);
        self.mirror_track(track);
        Ok(())
    }

    /// `AnimationTrack:AdjustWeight(weight, fadeTime)`. A track that is not
    /// playing keeps its weight: only `Play` brings it back.
    pub fn adjust_track_weight(&mut self, track: InstanceId, weight: f32, fade: f32) -> Result<(), String> {
        let now = self.frame.time;
        {
            let t = self.animation.tracks.get_mut(&track).ok_or("AdjustWeight on an unloaded AnimationTrack")?;
            let c = &mut t.control;
            if !c.playing {
                return Ok(());
            }
            let from = c.weight_at(now);
            c.fade_start = now;
            c.fade_from = from;
            c.fade_to = finite_or(weight, 1.0).max(0.0);
            c.fade_secs = finite_or(fade, DEFAULT_FADE).max(0.0);
        }
        self.log_control(track);
        self.mirror_track(track);
        Ok(())
    }

    /// A script's write to an `AnimationTrack` property. `DataModel::set_prop`
    /// routes every AnimationTrack write here, after `Name`, `Parent` and
    /// `Archivable`.
    pub fn set_track_prop(&mut self, track: InstanceId, name: &str, value: DmValue) -> Result<(), String> {
        let now = self.frame.time;
        match name {
            "Priority" => {
                let p = Priority::from_value(&value)
                    .ok_or_else(|| format!("Priority expects an Enum.AnimationPriority, got {}", value.type_name()))?;
                let t = self.animation.tracks.get_mut(&track).ok_or("AnimationTrack is not loaded")?;
                t.control.priority = p;
                t.priority_set = true;
            }
            "Looped" => {
                let b = value.as_bool().ok_or_else(|| format!("Looped expects a boolean, got {}", value.type_name()))?;
                let t = self.animation.tracks.get_mut(&track).ok_or("AnimationTrack is not loaded")?;
                t.control.looped = b;
                t.looped_set = true;
            }
            "TimePosition" => {
                let n = value
                    .as_number()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| format!("TimePosition expects a number, got {}", value.type_name()))?;
                let len = {
                    let t = self.animation.tracks.get(&track).ok_or("AnimationTrack is not loaded")?;
                    self.clip_length(t)
                };
                let t = self.animation.tracks.get_mut(&track).ok_or("AnimationTrack is not loaded")?;
                let c = &mut t.control;
                let at = clip_time(n, len, c.looped).0;
                c.anchor = now;
                c.anchor_time = at;
                t.last_raw = at as f64;
                t.include_start = false;
            }
            "IsPlaying" | "Length" | "Speed" | "WeightCurrent" | "WeightTarget" | "Animation" => {
                return Err(format!("Unable to assign property {name}. Property is read only"));
            }
            _ => return Err(format!("{name} is not a valid member of AnimationTrack")),
        }
        self.log_control(track);
        self.mirror_track(track);
        Ok(())
    }

    /// `KeyframeSequenceProvider:RegisterKeyframeSequence(sequence)`: an
    /// `active://` id an Animation's `AnimationId` can name, so a script can
    /// play a sequence it built. The same sequence keeps its id.
    pub fn register_keyframe_sequence(&mut self, sequence: InstanceId) -> Result<String, String> {
        match self.class_of(sequence) {
            Some("KeyframeSequence") => {}
            Some(other) => return Err(format!("RegisterKeyframeSequence expects a KeyframeSequence, got {other}")),
            None => return Err("RegisterKeyframeSequence expects a KeyframeSequence, got a destroyed instance".into()),
        }
        let known = self.animation.registered.iter().find(|(_, id)| **id == sequence).map(|(n, _)| *n);
        let n = match known {
            Some(n) => n,
            None => {
                self.animation.next_registered += 1;
                let n = self.animation.next_registered;
                self.animation.registered.insert(n, sequence);
                n
            }
        };
        Ok(format!("active://{n}"))
    }

    /// The sequence an `active://` id names, while it exists.
    pub fn registered_sequence(&self, content: &str) -> Option<InstanceId> {
        let n: u64 = content.trim().strip_prefix("active://")?.parse().ok()?;
        self.animation.registered.get(&n).copied().filter(|id| self.exists(*id))
    }

    /// `AnimationTrack:GetTimeOfKeyframe(name)`: the first keyframe of that
    /// name, or 0 when there is none or the clip has not loaded.
    pub fn time_of_keyframe(&self, track: InstanceId, name: &str) -> f64 {
        let Some(t) = self.animation.tracks.get(&track) else { return 0.0 };
        self.animation
            .clips
            .get(&(t.animator, t.content.clone()))
            .and_then(|c| c.keyframes.iter().find(|(_, n)| n == name))
            .map_or(0.0, |(time, _)| *time as f64)
    }

    /// `AnimationTrack:Destroy()` and a vanished Animator: the track goes.
    pub fn unload_track(&mut self, track: InstanceId) {
        let Some(t) = self.animation.tracks.remove(&track) else { return };
        if let Some(list) = self.animation.by_animator.get_mut(&t.animator) {
            list.retain(|id| *id != track);
            if list.is_empty() {
                self.animation.by_animator.remove(&t.animator);
            }
        }
        if self.networked && !t.remote {
            self.animation.ops.push(TrackOp::Unload { track });
        }
        if self.exists(track) {
            self.destroy(track);
        }
    }

    /// A replicated control state. Raises `AnimationPlayed` and `Stopped`
    /// when the sender played or stopped, and logs nothing.
    pub fn set_remote_track_control(&mut self, track: InstanceId, control: TrackControl) {
        let now = self.frame.time;
        let (animator, played, stopped) = {
            let Some(t) = self.animation.tracks.get_mut(&track) else { return };
            let old = t.control;
            t.control = control;
            let played = control.plays != old.plays;
            let stopped = control.stops != old.stops && old.playing;
            if played {
                // Events since the sender's Play fire here too, from its start.
                t.last_raw = control.anchor_time as f64;
                t.include_start = true;
                t.ending = false;
            } else {
                t.last_raw = control.raw_time_at(now);
                t.include_start = false;
            }
            if old.playing && !control.playing {
                t.ending = true;
            }
            (t.animator, played, stopped)
        };
        if played {
            self.push_event(signal(animator, "AnimationPlayed", vec![DmValue::Instance(track)]));
            self.echo_played_on_owner(animator, track);
        }
        if stopped {
            self.push_track_event(signal(track, "Stopped", Vec::new()));
        }
        self.mirror_track(track);
    }

    /// The evaluator's report on a clip it loaded for `animator`.
    pub fn set_clip_info(&mut self, animator: InstanceId, content: &str, info: ClipInfo) {
        let now = self.frame.time;
        self.animation.clips.insert((animator, content.to_string()), info);
        let ids = self.animation.by_animator.get(&animator).cloned().unwrap_or_default();
        for id in ids {
            let first_time = self
                .animation
                .tracks
                .get(&id)
                .is_some_and(|t| t.content == content && !t.clip_applied);
            if first_time {
                self.apply_clip_to_track(id);
                // Events start counting from now: a clip that loads late does
                // not replay every keyframe it missed while loading.
                if let Some(t) = self.animation.tracks.get_mut(&id) {
                    if !t.include_start {
                        t.last_raw = t.control.raw_time_at(now);
                    }
                }
                self.mirror_track(id);
            }
        }
    }

    pub fn clip_info(&self, animator: InstanceId, content: &str) -> Option<&ClipInfo> {
        self.animation.clips.get(&(animator, content.to_string()))
    }

    /// Give a track its clip's `Loop` and `Priority`, unless the script set them.
    fn apply_clip_to_track(&mut self, track: InstanceId) {
        let info = {
            let Some(t) = self.animation.tracks.get(&track) else { return };
            if t.clip_applied {
                return;
            }
            match self.animation.clips.get(&(t.animator, t.content.clone())) {
                Some(c) => c.clone(),
                None => return,
            }
        };
        if let Some(t) = self.animation.tracks.get_mut(&track) {
            if !t.looped_set {
                t.control.looped = info.looped;
            }
            if !t.priority_set {
                t.control.priority = info.priority;
            }
            t.clip_applied = true;
        }
    }

    /// Raise a track signal, and keep it in this frame's list for scripts
    /// that poll.
    fn push_track_event(&mut self, e: DmEvent) {
        if let Some(event) = TrackEvent::of(&e) {
            self.animation.frame_events.push(event);
        }
        self.push_event(e);
    }

    /// Record a control change for the session.
    fn log_control(&mut self, track: InstanceId) {
        if !self.networked {
            return;
        }
        if let Some(t) = self.animation.tracks.get(&track).filter(|t| !t.remote) {
            self.animation.ops.push(TrackOp::Control { track, control: t.control });
        }
    }

    /// Changes scripts made since the last call, with only the newest
    /// `Control` kept per track (at its latest place in the order).
    pub fn take_track_ops(&mut self) -> Vec<TrackOp> {
        let ops = std::mem::take(&mut self.animation.ops);
        let mut last_control: BTreeMap<InstanceId, usize> = BTreeMap::new();
        for (i, op) in ops.iter().enumerate() {
            if let TrackOp::Control { track, .. } = op {
                last_control.insert(*track, i);
            }
        }
        ops.into_iter()
            .enumerate()
            .filter(|(i, op)| match op {
                TrackOp::Control { track, .. } => last_control.get(track) == Some(i),
                _ => true,
            })
            .map(|(_, op)| op)
            .collect()
    }

    /// Write a track's read-only properties from its control state. Engine
    /// writes: nothing is marked dirty, and `Changed` fires for listeners.
    fn mirror_track(&mut self, track: InstanceId) {
        let now = self.frame.time;
        let Some(t) = self.animation.tracks.get(&track) else { return };
        let len = self.clip_length(t);
        let c = t.control;
        let values = [
            ("IsPlaying", DmValue::Bool(c.playing)),
            ("Length", DmValue::Number(len as f64)),
            ("Looped", DmValue::Bool(c.looped)),
            ("Priority", DmValue::Enum(c.priority.enum_item())),
            ("Speed", DmValue::Number(c.speed as f64)),
            ("TimePosition", DmValue::Number(c.time_at(now, len) as f64)),
            ("WeightCurrent", DmValue::Number(c.weight_at(now) as f64)),
            ("WeightTarget", DmValue::Number(c.fade_to as f64)),
        ];
        for (name, value) in values {
            self.set_prop_from_engine(track, name, value);
        }
    }

    /// Advance every track to the clock: raise `Stopped` at a clip's end,
    /// `Ended` once a fade-out finishes, `DidLoop`, `KeyframeReached` and
    /// markers, and update the read-only properties. Runs once a frame, after
    /// the clock advances and before scripts.
    pub fn step_animation(&mut self) {
        let now = self.frame.time;
        self.animation.frame_events.clear();

        // Tracks whose instance or Animator is gone go first.
        let gone: Vec<InstanceId> = self
            .animation
            .tracks
            .iter()
            .filter(|(id, t)| !self.exists(**id) || !self.exists(t.animator))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.unload_track(id);
        }
        // Clip facts outlive a track, so a new track on the same Animator
        // starts with them; they go with the Animator.
        let dead_clips: Vec<(InstanceId, String)> =
            self.animation.clips.keys().filter(|k| !self.exists(k.0)).cloned().collect();
        for key in dead_clips {
            self.animation.clips.remove(&key);
        }
        let dead_warned: Vec<InstanceId> =
            self.animation.warned_full.iter().filter(|a| !self.exists(**a)).copied().collect();
        for a in dead_warned {
            self.animation.warned_full.remove(&a);
        }
        let dead_registered: Vec<u64> =
            self.animation.registered.iter().filter(|(_, ks)| !self.exists(**ks)).map(|(n, _)| *n).collect();
        for n in dead_registered {
            self.animation.registered.remove(&n);
        }
        let dead_humanoids: Vec<InstanceId> =
            self.animation.humanoids.keys().filter(|h| !self.exists(**h)).copied().collect();
        for h in dead_humanoids {
            self.animation.humanoids.remove(&h);
        }

        let ids: Vec<InstanceId> = self.animation.tracks.keys().copied().collect();
        let mut events: Vec<DmEvent> = Vec::new();
        for id in ids {
            let info = {
                let Some(t) = self.animation.tracks.get(&id) else { continue };
                self.animation.clips.get(&(t.animator, t.content.clone())).cloned()
            };
            let Some(t) = self.animation.tracks.get_mut(&id) else { continue };
            step_track(id, t, info.as_ref(), now, &mut events);
        }
        for e in events {
            self.push_track_event(e);
        }
        let ids: Vec<InstanceId> = self.animation.tracks.keys().copied().collect();
        for id in ids {
            self.mirror_track(id);
        }
    }

    /// What the evaluator draws this frame, per Animator.
    pub fn animation_frame(&self) -> Vec<AnimatorFrame> {
        let now = self.frame.time;
        let mut out = Vec::new();
        for (animator, ids) in &self.animation.by_animator {
            let mut tracks = Vec::new();
            for id in ids {
                let Some(t) = self.animation.tracks.get(id) else { continue };
                if t.inert {
                    continue;
                }
                let weight = t.control.weight_at(now);
                if !t.control.playing && weight <= WEIGHT_EPSILON {
                    continue;
                }
                let len = self.clip_length(t);
                tracks.push(TrackSample {
                    track: *id,
                    content: t.content.clone(),
                    time: t.control.time_at(now, len),
                    weight: if weight <= WEIGHT_EPSILON { 0.0 } else { weight },
                    priority: t.control.priority,
                });
            }
            tracks.sort_by_key(|s| s.track);
            out.push(AnimatorFrame { animator: *animator, tracks });
        }
        out
    }
}

/// Advance one track to `now`, collecting the events it raises.
fn step_track(id: InstanceId, t: &mut Track, clip: Option<&ClipInfo>, now: f64, events: &mut Vec<DmEvent>) {
    let len = clip.map_or(0.0, |c| c.length);
    let c = &mut t.control;
    let mut raw = c.raw_time_at(now);

    // A non-looped clip that reaches its end stops there and fades out.
    let mut reached_end = false;
    if c.playing && !c.looped && len > 0.0 && !c.held {
        let boundary = if c.speed >= 0.0 { len as f64 } else { 0.0 };
        let crossed = if c.speed > 0.0 {
            raw >= boundary
        } else if c.speed < 0.0 {
            raw <= boundary
        } else {
            false
        };
        if crossed {
            let end_at = c.anchor + (boundary - c.anchor_time as f64) / c.speed as f64;
            let end_at = end_at.clamp(c.anchor, now);
            let w = c.weight_at(end_at);
            c.playing = false;
            c.held = true;
            c.anchor = end_at;
            c.anchor_time = boundary as f32;
            c.fade_start = end_at;
            c.fade_from = w;
            c.fade_to = 0.0;
            c.fade_secs = c.end_fade;
            raw = boundary;
            reached_end = true;
            t.ending = true;
        }
    }

    // Keyframes, markers and loops crossed since the last step, in playback
    // order, while the track still shows. Right after `Play` the starting
    // instant counts too.
    let showing = c.playing || reached_end || c.weight_at(now) > WEIGHT_EPSILON;
    if let Some(info) = clip.filter(|i| i.length > 0.0 && showing) {
        let passed = crossings(t.last_raw, raw, t.include_start, info, c.looped);
        for crossing in passed.into_iter().take(MAX_CROSSINGS_PER_STEP) {
            match crossing {
                Crossing::Loop => events.push(signal(id, "DidLoop", Vec::new())),
                Crossing::Keyframe(name) => {
                    if name != DEFAULT_KEYFRAME_NAME {
                        events.push(signal(id, "KeyframeReached", vec![DmValue::String(name)]));
                    }
                }
                Crossing::Marker(name, value) => {
                    events.push(signal(id, format!("Marker:{name}"), vec![DmValue::String(value)]));
                }
            }
        }
    }
    t.last_raw = raw;
    t.include_start = false;

    if reached_end {
        events.push(signal(id, "Stopped", Vec::new()));
    }

    // `Ended` once a stopped track has faded out.
    if t.ending && !c.playing && c.weight_at(now) <= WEIGHT_EPSILON {
        t.ending = false;
        events.push(signal(id, "Ended", Vec::new()));
    }
}

/// Something playback passed between two steps.
#[derive(Debug, Clone, PartialEq)]
enum Crossing {
    Loop,
    Keyframe(String),
    Marker(String, String),
}

/// The keyframes, markers and loop points an unwrapped time passed on its
/// way from `from` to `to`, in the order it passed them. `include_start`
/// counts the instant at `from` as passed: playback just started there.
fn crossings(from: f64, to: f64, include_start: bool, info: &ClipInfo, looped: bool) -> Vec<Crossing> {
    const EPS: f64 = 1e-9;
    let len = info.length as f64;
    if !(len > 0.0) || (!include_start && (to - from).abs() < EPS) {
        return Vec::new();
    }
    let forward = to >= from;
    // A point is passed when it lies after `from` (or at it, on the starting
    // instant) and no later than `to`, along the direction of playback.
    let passed = |x: f64| -> bool {
        if forward {
            let after = if include_start { x >= from - EPS } else { x > from + EPS };
            after && x <= to + EPS
        } else {
            let after = if include_start { x <= from + EPS } else { x < from - EPS };
            after && x >= to - EPS
        }
    };

    let cycles: Vec<i64> = if looped {
        let (lo, hi) = if forward { (from, to) } else { (to, from) };
        let first = (lo / len).floor() as i64;
        let last = ((hi / len).floor() as i64).min(first + MAX_LOOPS_PER_STEP + 1);
        (first..=last).collect()
    } else {
        vec![0]
    };

    // (position, order at that position, what happened). At a cycle
    // boundary playing forward, the end of one cycle comes first, then the
    // loop, then the start of the next; playing backward, the reverse.
    let mut points: Vec<(f64, u8, Crossing)> = Vec::new();
    let order_of = |time: f32| if (time as f64 - len).abs() < EPS { 0u8 } else { 2u8 };
    for n in cycles {
        let base = n as f64 * len;
        for (time, name) in &info.keyframes {
            let x = base + *time as f64;
            if passed(x) {
                points.push((x, order_of(*time), Crossing::Keyframe(name.clone())));
            }
        }
        for (time, name, value) in &info.markers {
            let x = base + *time as f64;
            if passed(x) {
                points.push((x, order_of(*time), Crossing::Marker(name.clone(), value.clone())));
            }
        }
        if looped {
            // Every whole cycle is a loop, except the instant playback started.
            let x = base;
            let at_start = include_start && (x - from).abs() < EPS;
            if !at_start && passed(x) {
                points.push((x, 1, Crossing::Loop));
            }
        }
    }
    if forward {
        points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    } else {
        points.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    }
    let mut loops = 0i64;
    points
        .into_iter()
        .filter(|(_, _, c)| {
            if *c == Crossing::Loop {
                loops += 1;
                loops <= MAX_LOOPS_PER_STEP
            } else {
                true
            }
        })
        .map(|(_, _, c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig(dm: &mut DataModel) -> (InstanceId, InstanceId) {
        let ws = dm.get_service("Workspace").expect("Workspace");
        let model = dm.create_virtual("Model", "Character", Some(ws));
        let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
        let animator = dm.create_virtual("Animator", "Animator", Some(humanoid));
        (humanoid, animator)
    }

    fn animation(dm: &mut DataModel, id: &str) -> InstanceId {
        let a = dm.create("Animation");
        dm.set_prop(a, "AnimationId", DmValue::String(id.into())).unwrap();
        a
    }

    fn clip(length: f32, looped: bool) -> ClipInfo {
        ClipInfo {
            length,
            looped,
            priority: Priority::Action,
            keyframes: vec![(0.0, "Keyframe".into()), (0.5, "Mid".into()), (length, "End".into())],
            markers: vec![(0.25, "Step".into(), "Left".into())],
        }
    }

    fn signals(dm: &DataModel) -> Vec<(InstanceId, String)> {
        dm.events_since(0)
            .0
            .into_iter()
            .filter_map(|e| match e {
                DmEvent::Signal { id, name, .. } => Some((id, name)),
                _ => None,
            })
            .collect()
    }

    fn at(dm: &mut DataModel, time: f64) {
        dm.frame.time = time;
        dm.step_animation();
    }

    #[test]
    fn priority_names_and_roblox_values_round_trip() {
        for p in Priority::HIGHEST_FIRST {
            assert_eq!(Priority::from_name(p.name()), Some(p));
            assert_eq!(Priority::from_roblox_value(p.roblox_value()), Some(p));
        }
        assert_eq!(Priority::from_name("Enum.AnimationPriority.Action2"), Some(Priority::Action2));
        assert!(Priority::Core < Priority::Idle && Priority::Action3 < Priority::Action4);
        assert_eq!(Priority::Core.roblox_value(), 1000);
    }

    #[test]
    fn four_same_priority_tracks_at_half_share_evenly() {
        let (w, rest) = effective_weights(&[(Priority::Core, 0.5); 4]);
        for x in &w {
            assert!((x - 0.25).abs() < 1e-6, "{w:?}");
        }
        assert_eq!(rest, 0.0);
    }

    #[test]
    fn a_higher_priority_takes_its_weight_and_leaves_the_rest() {
        let (w, rest) = effective_weights(&[(Priority::Movement, 1.0), (Priority::Action, 0.3)]);
        assert!((w[1] - 0.3).abs() < 1e-6 && (w[0] - 0.7).abs() < 1e-6, "{w:?}");
        assert_eq!(rest, 0.0);
    }

    #[test]
    fn a_full_weight_action_leaves_nothing_below_it() {
        let (w, rest) = effective_weights(&[(Priority::Movement, 1.0), (Priority::Action, 1.0)]);
        assert_eq!(w, vec![0.0, 1.0]);
        assert_eq!(rest, 0.0);
    }

    #[test]
    fn a_lone_partial_track_lies_between_its_pose_and_rest() {
        let (w, rest) = effective_weights(&[(Priority::Action, 0.5)]);
        assert!((w[0] - 0.5).abs() < 1e-6 && (rest - 0.5).abs() < 1e-6);
        let (w, rest) = effective_weights(&[]);
        assert!(w.is_empty() && rest == 1.0);
    }

    #[test]
    fn crossfading_priorities_never_leak_the_rest_pose() {
        // Idle fading out under a walk fading in, both over the same 0.2 s.
        for step in 0..=10 {
            let s = step as f32 / 10.0;
            let (w, rest) = effective_weights(&[(Priority::Idle, 1.0 - s), (Priority::Movement, s)]);
            assert!(rest.abs() < 1e-5, "rest {rest} at {s}: {w:?}");
            assert!((w[0] + w[1] - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn zero_and_non_finite_weights_contribute_exactly_nothing() {
        let (w, rest) = effective_weights(&[(Priority::Core, 0.0), (Priority::Core, f32::NAN), (Priority::Core, 1e-9)]);
        assert_eq!(w, vec![0.0, 0.0, 0.0]);
        assert_eq!(rest, 1.0);
    }

    #[test]
    fn play_fades_in_and_stop_fades_out_then_ends() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "space://Wave.anim.toml");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "space://Wave.anim.toml", clip(2.0, true));
        assert_eq!(dm.get_prop(track, "Length"), Some(DmValue::Number(2.0)));

        dm.frame.time = 10.0;
        dm.play_track(track, 0.2, 1.0, 1.0).unwrap();
        assert_eq!(dm.get_prop(track, "IsPlaying"), Some(DmValue::Bool(true)));
        at(&mut dm, 10.1);
        let w = dm.get_prop(track, "WeightCurrent").and_then(|v| v.as_number()).unwrap();
        assert!((w - 0.5).abs() < 1e-4, "half way through the fade: {w}");

        dm.stop_track(track, 0.1).unwrap();
        assert_eq!(dm.get_prop(track, "IsPlaying"), Some(DmValue::Bool(false)));
        at(&mut dm, 10.25);
        let names: Vec<String> = signals(&dm).into_iter().filter(|(id, _)| *id == track).map(|(_, n)| n).collect();
        assert!(names.contains(&"Stopped".to_string()), "{names:?}");
        assert!(names.contains(&"Ended".to_string()), "{names:?}");
        let stopped = names.iter().position(|n| n == "Stopped").unwrap();
        let ended = names.iter().position(|n| n == "Ended").unwrap();
        assert!(stopped < ended);
        assert!(dm.playing_tracks(humanoid).is_empty());
    }

    #[test]
    fn a_non_looped_clip_ends_itself_and_holds_its_last_frame() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "space://Jump.anim.toml");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "space://Jump.anim.toml", clip(1.0, false));
        dm.frame.time = 0.0;
        dm.play_track(track, 0.0, 1.0, 1.0).unwrap();
        at(&mut dm, 1.5);
        assert_eq!(dm.get_prop(track, "IsPlaying"), Some(DmValue::Bool(false)));
        assert_eq!(dm.get_prop(track, "TimePosition"), Some(DmValue::Number(1.0)));
        let names: Vec<String> = signals(&dm).into_iter().map(|(_, n)| n).collect();
        assert!(names.iter().any(|n| n == "Stopped"), "{names:?}");
        assert!(names.iter().any(|n| n == "KeyframeReached"), "{names:?}");
        // The fade-out uses the Play fade (0 here), so it has already ended.
        assert!(names.iter().any(|n| n == "Ended"), "{names:?}");
        at(&mut dm, 3.0);
        assert_eq!(dm.get_prop(track, "TimePosition"), Some(DmValue::Number(1.0)), "holds the last frame");
    }

    #[test]
    fn the_frame_list_holds_this_frames_track_events_only() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "rig://walk");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "rig://walk", clip(1.0, true));
        dm.frame.time = 0.0;
        dm.play_track(track, 0.0, 1.0, 1.0).unwrap();
        at(&mut dm, 0.3);
        let kinds: Vec<(&str, String, String)> =
            dm.animation.frame_events().iter().map(|e| (e.kind, e.name.clone(), e.value.clone())).collect();
        assert_eq!(kinds, vec![("marker", "Step".to_string(), "Left".to_string())], "the unnamed first keyframe is quiet");
        at(&mut dm, 0.4);
        assert!(dm.animation.frame_events().is_empty(), "a frame that crosses nothing lists nothing");
        dm.stop_track(track, 0.2).unwrap();
        assert_eq!(dm.animation.frame_events()[0].kind, "stopped", "a script's Stop joins this frame's list");
        at(&mut dm, 1.0);
        assert!(dm.animation.frame_events().iter().any(|e| e.kind == "ended" && e.track == track));
    }

    #[test]
    fn looping_fires_did_loop_and_markers_in_order() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "rig://walk");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "rig://walk", clip(1.0, true));
        dm.frame.time = 0.0;
        dm.play_track(track, 0.0, 1.0, 1.0).unwrap();
        at(&mut dm, 0.3);
        at(&mut dm, 1.3);
        let names: Vec<String> =
            signals(&dm).into_iter().filter(|(id, _)| *id == track).map(|(_, n)| n).collect();
        let loop_at = names.iter().position(|n| n == "DidLoop").expect("DidLoop");
        let second_step = names.iter().rposition(|n| n == "Marker:Step").unwrap();
        assert!(loop_at < second_step, "{names:?}");
        assert_eq!(names.iter().filter(|n| *n == "Marker:Step").count(), 2, "{names:?}");
        let t = dm.get_prop(track, "TimePosition").and_then(|v| v.as_number()).unwrap();
        assert!((t - 0.3).abs() < 1e-4, "{t}");
    }

    #[test]
    fn negative_speed_plays_backward_from_the_end() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "rig://idle");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "rig://idle", clip(2.0, false));
        dm.frame.time = 0.0;
        dm.play_track(track, 0.0, 1.0, -1.0).unwrap();
        at(&mut dm, 0.5);
        let t = dm.get_prop(track, "TimePosition").and_then(|v| v.as_number()).unwrap();
        assert!((t - 1.5).abs() < 1e-4, "{t}");
        at(&mut dm, 3.0);
        assert_eq!(dm.get_prop(track, "TimePosition"), Some(DmValue::Number(0.0)));
        assert_eq!(dm.get_prop(track, "IsPlaying"), Some(DmValue::Bool(false)));
    }

    #[test]
    fn a_seek_skips_the_keyframes_between() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "rig://run");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_clip_info(animator, "rig://run", clip(1.0, true));
        dm.frame.time = 0.0;
        dm.play_track(track, 0.0, 1.0, 1.0).unwrap();
        at(&mut dm, 0.1);
        dm.set_prop(track, "TimePosition", DmValue::Number(0.9)).unwrap();
        at(&mut dm, 0.15);
        let markers = signals(&dm).into_iter().filter(|(_, n)| n == "Marker:Step").count();
        assert_eq!(markers, 0);
    }

    #[test]
    fn script_set_priority_survives_the_clip_loading() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let anim = animation(&mut dm, "space://Emote.anim.toml");
        let track = dm.load_animation(humanoid, anim).unwrap();
        dm.set_prop(track, "Priority", DmValue::Enum(Priority::Movement.enum_item())).unwrap();
        dm.set_clip_info(animator, "space://Emote.anim.toml", clip(1.0, true));
        assert_eq!(dm.get_prop(track, "Priority"), Some(DmValue::Enum(Priority::Movement.enum_item())));
        assert_eq!(dm.get_prop(track, "Looped"), Some(DmValue::Bool(true)), "the clip's Loop still applies");
        assert!(dm.set_prop(track, "IsPlaying", DmValue::Bool(true)).is_err());
    }

    #[test]
    fn humanoid_load_animation_makes_one_animator() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let model = dm.create_virtual("Model", "Npc", Some(ws));
        let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
        let a = animation(&mut dm, "rig://wave");
        dm.load_animation(humanoid, a).unwrap();
        dm.load_animation(humanoid, a).unwrap();
        let animators = dm.children(humanoid).iter().filter(|c| dm.class_of(**c) == Some("Animator")).count();
        assert_eq!(animators, 1);
        let part = dm.create_virtual("Part", "Part", Some(ws));
        assert!(dm.load_animation(part, a).is_err());
    }

    #[test]
    fn the_track_limit_releases_the_oldest_finished_track() {
        let mut dm = DataModel::new();
        let (humanoid, _) = rig(&mut dm);
        let a = animation(&mut dm, "rig://idle");
        let first = dm.load_animation(humanoid, a).unwrap();
        for _ in 1..MAX_TRACKS_PER_ANIMATOR {
            dm.load_animation(humanoid, a).unwrap();
        }
        let extra = dm.load_animation(humanoid, a).unwrap();
        assert!(!dm.is_track(first), "the oldest finished track was released");
        assert!(dm.is_track(extra) && !dm.animation.track(extra).unwrap().is_inert());
        assert_eq!(dm.tracks_of(humanoid).len(), MAX_TRACKS_PER_ANIMATOR);
    }

    #[test]
    fn two_trees_given_the_same_control_agree_at_every_time() {
        let mut host = DataModel::new();
        let mut player = DataModel::new();
        let (h1, a1) = rig(&mut host);
        let (_, a2) = rig(&mut player);
        let anim = animation(&mut host, "rig://walk");
        let t1 = host.load_animation(h1, anim).unwrap();
        let t2 = player.load_remote_track(a2, None, "rig://walk", "walk");
        host.set_clip_info(a1, "rig://walk", clip(1.3, true));
        player.set_clip_info(a2, "rig://walk", clip(1.3, true));
        host.frame.time = 5.0;
        host.play_track(t1, 0.3, 0.8, 1.25).unwrap();
        let control = host.animation.track(t1).unwrap().control;
        player.set_remote_track_control(t2, control);
        for i in 0..40 {
            let now = 5.0 + i as f64 * 0.05;
            at(&mut host, now);
            at(&mut player, now);
            assert_eq!(host.get_prop(t1, "TimePosition"), player.get_prop(t2, "TimePosition"), "at {now}");
            assert_eq!(host.get_prop(t1, "WeightCurrent"), player.get_prop(t2, "WeightCurrent"), "at {now}");
        }
        let played = signals(&player).into_iter().any(|(id, n)| id == a2 && n == "AnimationPlayed");
        assert!(played, "a replicated Play raises AnimationPlayed on the receiver");
    }

    #[test]
    fn ops_are_logged_only_while_networked_and_coalesced() {
        let mut dm = DataModel::new();
        let (humanoid, _) = rig(&mut dm);
        let a = animation(&mut dm, "rig://idle");
        let quiet = dm.load_animation(humanoid, a).unwrap();
        dm.play_track(quiet, 0.1, 1.0, 1.0).unwrap();
        assert!(dm.take_track_ops().is_empty());

        dm.networked = true;
        let track = dm.load_animation(humanoid, a).unwrap();
        dm.play_track(track, 0.1, 1.0, 1.0).unwrap();
        dm.adjust_track_speed(track, 2.0).unwrap();
        dm.adjust_track_weight(track, 0.5, 0.1).unwrap();
        let ops = dm.take_track_ops();
        assert!(matches!(ops[0], TrackOp::Load { .. }));
        let controls = ops.iter().filter(|o| matches!(o, TrackOp::Control { .. })).count();
        assert_eq!(controls, 1, "{ops:?}");
        if let Some(TrackOp::Control { control, .. }) = ops.last() {
            assert_eq!(control.speed, 2.0);
            assert_eq!(control.fade_to, 0.5);
        }
    }

    #[test]
    fn a_track_the_session_loaded_never_echoes() {
        let mut dm = DataModel::new();
        let (_, animator) = rig(&mut dm);
        dm.networked = true;
        let track = dm.load_remote_track(animator, None, "rig://walk", "walk");
        assert!(dm.animation.track(track).is_some_and(|t| t.is_remote()));
        dm.play_track(track, 0.1, 1.0, 1.0).unwrap();
        dm.unload_track(track);
        assert!(dm.take_track_ops().is_empty(), "a remote track's changes stay on this machine");
    }

    #[test]
    fn a_destroyed_animator_unloads_its_tracks() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let a = animation(&mut dm, "rig://idle");
        let track = dm.load_animation(humanoid, a).unwrap();
        dm.destroy(animator);
        at(&mut dm, 1.0);
        assert!(!dm.is_track(track));
        assert!(dm.animation_frame().is_empty());
    }

    #[test]
    fn the_frame_lists_playing_and_fading_tracks_only() {
        let mut dm = DataModel::new();
        let (humanoid, animator) = rig(&mut dm);
        let a = animation(&mut dm, "rig://idle");
        let playing = dm.load_animation(humanoid, a).unwrap();
        let _idle = dm.load_animation(humanoid, a).unwrap();
        dm.set_clip_info(animator, "rig://idle", clip(1.0, true));
        dm.play_track(playing, 0.0, 0.0, 1.0).unwrap();
        let frame = dm.animation_frame();
        assert_eq!(frame.len(), 1);
        assert_eq!(frame[0].tracks.len(), 1, "a playing track at weight 0 keeps its place");
        assert_eq!(frame[0].tracks[0].weight, 0.0);
    }
}

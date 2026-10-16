//! # The pose database
//!
//! Every frame of every clip becomes one row of [`features`](super::features),
//! built once, in the character's frame, from what the clip says about its own
//! feet and root. The matcher then scans the rows for the one that fits the
//! character best.
//!
//! The input is plain frames ([`ClipFrames`]): where each foot and the hips
//! are in the root's frame on each frame, and how the root moves. Turning a
//! glTF clip or a clip record into frames is the sampler's job, not this
//! module's, which keeps the build testable on synthetic clips.
//!
//! **Root motion.** Most bundled clips are in place, so a clip states its root
//! motion as data: a speed, the direction of travel against the facing, and a
//! turning rate ([`ClipFrames::with_constant_root`]). A clip that carries real
//! root motion supplies it frame by frame.
//!
//! **Normalization.** Each value has its mean removed, and each group is divided
//! by its own spread (never below a floor in the group's units, so a group that
//! barely varies in the database does not turn a wobble into a shout). A group
//! counts in the cost as the mean squared difference of its values, times its
//! weight, so weights can change without a rebuild.
//!
//! **Contacts.** A foot is in contact on a frame when its speed against the
//! ground is small and its sole is near the clip's lowest foot height, smoothed
//! over three frames. The speed against the ground adds the root's own speed to
//! the foot's speed through the root frame: the planted foot of an in-place
//! walk moves backward through the body at ground speed, so its speed through
//! the body alone would never be small.

use bevy::math::{Vec2, Vec3};

use super::features::{FeatureLayout, FeatureWeights, Group, Query};
use super::trajectory::{forward, to_world, TrajectoryPoint};

/// How far above a clip's lowest foot a sole can be and still be planted, metres.
const CONTACT_HEIGHT: f32 = 0.05;

/// Ground speed under which a foot counts as planted: this, plus a fraction of
/// the root's speed, metres per second.
const CONTACT_SPEED: f32 = 0.2;
const CONTACT_SPEED_PER_ROOT_SPEED: f32 = 0.08;

/// Least spread a group is divided by, in the group's own units.
const MIN_SCALE: [f32; 5] = [0.1, 0.2, 0.05, 0.3, 0.3];

/// One frame of a clip, in the root's frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseFrame {
    pub left_foot: Vec3,
    pub right_foot: Vec3,
    /// The hips' height above the sole level, metres.
    pub hips_height: f32,
}

/// How the root moves on one frame. `velocity` is `(x, z)` in the root's own
/// frame, so forward is `-z`; `yaw_rate` is radians per second, positive turning
/// left.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RootMotion {
    pub velocity: Vec2,
    pub yaw_rate: f32,
}

/// A clip as the database builds from it.
#[derive(Debug, Clone)]
pub struct ClipFrames {
    pub name: String,
    pub fps: f32,
    pub looping: bool,
    pub tags: Vec<String>,
    pub frames: Vec<PoseFrame>,
    /// One per frame.
    pub root: Vec<RootMotion>,
}

impl ClipFrames {
    /// A clip whose root moves the same way on every frame: `speed` m/s along
    /// `heading` degrees from forward (90 is right, 180 backward) while turning
    /// `yaw_rate` degrees per second to the left.
    pub fn with_constant_root(mut self, speed: f32, heading_degrees: f32, yaw_rate_degrees: f32) -> Self {
        let h = heading_degrees.to_radians();
        let motion = RootMotion { velocity: Vec2::new(h.sin(), -h.cos()) * speed, yaw_rate: yaw_rate_degrees.to_radians() };
        self.root = vec![motion; self.frames.len()];
        self
    }

    /// Seconds the clip lasts. A looping clip ends where it starts, so its last
    /// frame is not a repeat of its first.
    pub fn duration(&self) -> f32 {
        let n = self.frames.len();
        if self.looping {
            n as f32 / self.fps
        } else {
            n.saturating_sub(1) as f32 / self.fps
        }
    }
}

/// What the database keeps about a clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipMeta {
    pub name: String,
    pub looping: bool,
    pub fps: f32,
    pub duration: f32,
    /// One bit per tag, in the order the tags first appear.
    pub tag_bits: u64,
    /// Row of the clip's first frame.
    pub first_frame: usize,
    pub frame_count: usize,
}

/// Where a row came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInfo {
    pub clip: usize,
    /// Frame within the clip.
    pub local: usize,
    /// Seconds into the clip.
    pub time: f32,
}

/// Which feet are planted on a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contacts {
    pub left: bool,
    pub right: bool,
}

const LEFT_BIT: u8 = 1;
const RIGHT_BIT: u8 = 2;

/// The frame before and after `i`, and the number of frames between them.
fn neighbours(i: usize, n: usize, looping: bool) -> (usize, usize, f32) {
    if looping {
        ((i + n - 1) % n, (i + 1) % n, 2.0)
    } else {
        let prev = i.saturating_sub(1);
        let next = (i + 1).min(n - 1);
        (prev, next, (next - prev) as f32)
    }
}

/// A frame index that wraps for a looping clip and holds the last frame for
/// another.
fn index_at(i: usize, n: usize, looping: bool) -> usize {
    if looping {
        i % n
    } else {
        i.min(n - 1)
    }
}

/// The root's path from frame `start` on, in the frame it is in at `start`, at
/// each of `times`. Past the end of a clip that does not loop, the last frame's
/// motion carries on.
fn clip_trajectory(clip: &ClipFrames, start: usize, times: &[f32]) -> Vec<TrajectoryPoint> {
    let n = clip.frames.len();
    let step = 1.0 / clip.fps;
    let last = times.last().copied().unwrap_or(0.0);
    let steps = (last / step).ceil() as usize;
    let mut path: Vec<(Vec2, f32)> = Vec::with_capacity(steps + 1);
    let (mut position, mut yaw) = (Vec2::ZERO, 0.0_f32);
    path.push((position, yaw));
    for k in 0..steps {
        let motion = clip.root[index_at(start + k, n, clip.looping)];
        position += to_world(motion.velocity, yaw) * step;
        yaw += motion.yaw_rate * step;
        path.push((position, yaw));
    }
    times
        .iter()
        .map(|&time| {
            let f = time / step;
            let i0 = (f.floor() as usize).min(steps);
            let i1 = (i0 + 1).min(steps);
            let a = f - i0 as f32;
            let (p0, y0) = path[i0];
            let (p1, y1) = path[i1];
            TrajectoryPoint { position: p0.lerp(p1, a), facing: forward(y0 + (y1 - y0) * a) }
        })
        .collect()
}

/// Whether a foot is planted, from its position, its speed through the root's
/// frame and the root's own motion.
fn is_planted(position: Vec3, velocity: Vec3, root: RootMotion, lowest: f32) -> bool {
    // The point of a turning body moves with the turn.
    let turning = Vec2::new(root.yaw_rate * position.z, -root.yaw_rate * position.x);
    let ground = Vec2::new(velocity.x, velocity.z) + root.velocity + turning;
    let limit = CONTACT_SPEED + CONTACT_SPEED_PER_ROOT_SPEED * root.velocity.length();
    ground.length() < limit && position.y < lowest + CONTACT_HEIGHT
}

/// The rows of every clip, ready to search.
#[derive(Debug, Clone, PartialEq)]
pub struct Database {
    layout: FeatureLayout,
    times: Vec<f32>,
    clips: Vec<ClipMeta>,
    tag_names: Vec<String>,
    /// Which clip each row belongs to.
    clip_of: Vec<u32>,
    /// Normalized rows, `dims` values each.
    features: Vec<f32>,
    contacts: Vec<u8>,
    /// Seconds left in the clip on each row; infinite for a looping clip.
    remaining: Vec<f32>,
    mean: Vec<f32>,
    scale: [f32; 5],
    weights: FeatureWeights,
    dim_weight: Vec<f32>,
    dim_group: Vec<usize>,
}

impl Database {
    /// Build the rows of `clips`, with the path sampled at `times` seconds
    /// ahead (ascending, as many as the layout says).
    pub fn build(
        clips: &[ClipFrames],
        layout: FeatureLayout,
        times: &[f32],
        weights: FeatureWeights,
    ) -> Result<Database, String> {
        if clips.is_empty() {
            return Err("a motion database needs at least one clip".into());
        }
        if times.len() != layout.trajectory_samples {
            return Err(format!("{} trajectory times for a layout of {}", times.len(), layout.trajectory_samples));
        }
        if times.iter().any(|t| !t.is_finite() || *t <= 0.0) || times.windows(2).any(|w| w[1] <= w[0]) {
            return Err("trajectory times must be positive and ascending".into());
        }
        let dims = layout.dims();
        let mut tag_names: Vec<String> = Vec::new();
        let mut metas: Vec<ClipMeta> = Vec::with_capacity(clips.len());
        let mut clip_of: Vec<u32> = Vec::new();
        let mut features: Vec<f32> = Vec::new();
        let mut contacts: Vec<u8> = Vec::new();
        let mut remaining: Vec<f32> = Vec::new();
        let mut row: Vec<f32> = Vec::with_capacity(dims);

        for (clip_index, clip) in clips.iter().enumerate() {
            let n = clip.frames.len();
            if !(clip.fps.is_finite() && clip.fps > 0.0) {
                return Err(format!("clip {:?}: a frame rate of {}", clip.name, clip.fps));
            }
            if n < 2 {
                return Err(format!("clip {:?}: {n} frame(s), need at least 2", clip.name));
            }
            if clip.root.len() != n {
                return Err(format!("clip {:?}: {} root motions for {n} frames", clip.name, clip.root.len()));
            }
            let finite = clip.frames.iter().all(|f| f.left_foot.is_finite() && f.right_foot.is_finite() && f.hips_height.is_finite())
                && clip.root.iter().all(|r| r.velocity.is_finite() && r.yaw_rate.is_finite());
            if !finite {
                return Err(format!("clip {:?}: a frame holds a value that is not a number", clip.name));
            }

            let mut tag_bits = 0_u64;
            for tag in &clip.tags {
                let tag = tag.trim();
                if tag.is_empty() {
                    continue;
                }
                let bit = match tag_names.iter().position(|t| t == tag) {
                    Some(bit) => bit,
                    None if tag_names.len() < 64 => {
                        tag_names.push(tag.to_string());
                        tag_names.len() - 1
                    }
                    None => return Err("a motion database holds at most 64 tags".into()),
                };
                tag_bits |= 1 << bit;
            }
            metas.push(ClipMeta {
                name: clip.name.clone(),
                looping: clip.looping,
                fps: clip.fps,
                duration: clip.duration(),
                tag_bits,
                first_frame: remaining.len(),
                frame_count: n,
            });

            let lowest = clip.frames.iter().flat_map(|f| [f.left_foot.y, f.right_foot.y]).fold(f32::INFINITY, f32::min);
            let mut planted = vec![0_u8; n];
            for i in 0..n {
                let (prev, next, steps) = neighbours(i, n, clip.looping);
                let dt = steps / clip.fps;
                let frame = clip.frames[i];
                let left_velocity = (clip.frames[next].left_foot - clip.frames[prev].left_foot) / dt;
                let right_velocity = (clip.frames[next].right_foot - clip.frames[prev].right_foot) / dt;
                let hips_velocity = (clip.frames[next].hips_height - clip.frames[prev].hips_height) / dt;
                let motion = clip.root[i];
                let query = Query {
                    trajectory: clip_trajectory(clip, i, times),
                    left_foot: frame.left_foot,
                    right_foot: frame.right_foot,
                    left_foot_velocity: left_velocity,
                    right_foot_velocity: right_velocity,
                    root_velocity: Vec3::new(motion.velocity.x, hips_velocity, motion.velocity.y),
                };
                query.write(layout, &mut row);
                features.extend_from_slice(&row);
                if is_planted(frame.left_foot, left_velocity, motion, lowest) {
                    planted[i] |= LEFT_BIT;
                }
                if is_planted(frame.right_foot, right_velocity, motion, lowest) {
                    planted[i] |= RIGHT_BIT;
                }
                clip_of.push(clip_index as u32);
                remaining.push(if clip.looping { f32::INFINITY } else { (n - 1 - i) as f32 / clip.fps });
            }
            // A foot is planted when two of the three frames around it say so.
            for i in 0..n {
                let (prev, next, _) = neighbours(i, n, clip.looping);
                let mut bits = 0_u8;
                for bit in [LEFT_BIT, RIGHT_BIT] {
                    let votes = [prev, i, next].iter().filter(|&&j| planted[j] & bit != 0).count();
                    if votes >= 2 {
                        bits |= bit;
                    }
                }
                contacts.push(bits);
            }
        }

        let frames = remaining.len();
        let mut mean = vec![0.0_f32; dims];
        for r in features.chunks(dims) {
            for (m, x) in mean.iter_mut().zip(r) {
                *m += x;
            }
        }
        for m in &mut mean {
            *m /= frames as f32;
        }
        let mut scale = [1.0_f32; 5];
        for group in Group::ALL {
            let range = layout.range(group);
            let mut sum = 0.0_f64;
            for r in features.chunks(dims) {
                for d in range.clone() {
                    let x = f64::from(r[d] - mean[d]);
                    sum += x * x;
                }
            }
            let count = (frames * range.len()).max(1) as f64;
            scale[group.index()] = ((sum / count).sqrt() as f32).max(MIN_SCALE[group.index()]);
        }
        let dim_group: Vec<usize> = (0..dims).map(|d| layout.group_of(d).index()).collect();
        for r in features.chunks_mut(dims) {
            for d in 0..dims {
                r[d] = (r[d] - mean[d]) / scale[dim_group[d]];
            }
        }

        let mut db = Database {
            layout,
            times: times.to_vec(),
            clips: metas,
            tag_names,
            clip_of,
            features,
            contacts,
            remaining,
            mean,
            scale,
            weights,
            dim_weight: Vec::new(),
            dim_group,
        };
        db.set_weights(weights);
        Ok(db)
    }

    pub fn layout(&self) -> FeatureLayout {
        self.layout
    }

    /// The seconds ahead the path is sampled at.
    pub fn times(&self) -> &[f32] {
        &self.times
    }

    pub fn dims(&self) -> usize {
        self.layout.dims()
    }

    pub fn frame_count(&self) -> usize {
        self.remaining.len()
    }

    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    pub fn clips(&self) -> &[ClipMeta] {
        &self.clips
    }

    pub fn weights(&self) -> FeatureWeights {
        self.weights
    }

    /// Change how much each group counts. No rebuild is needed.
    pub fn set_weights(&mut self, weights: FeatureWeights) {
        self.weights = weights;
        self.dim_weight = (0..self.dims())
            .map(|d| {
                let group = self.layout.group_of(d);
                let width = self.layout.range(group).len().max(1);
                weights.of(group) / width as f32
            })
            .collect();
    }

    /// The tags, in the order their bits are assigned.
    pub fn tag_names(&self) -> &[String] {
        &self.tag_names
    }

    /// The bit a tag has in [`ClipMeta::tag_bits`].
    pub fn tag_bit(&self, tag: &str) -> Option<u64> {
        self.tag_names.iter().position(|t| t == tag.trim()).map(|i| 1_u64 << i)
    }

    /// Which clip and time a row is.
    pub fn frame_info(&self, frame: usize) -> FrameInfo {
        let clip = self.clip_of[frame] as usize;
        let meta = &self.clips[clip];
        let local = frame - meta.first_frame;
        FrameInfo { clip, local, time: local as f32 / meta.fps }
    }

    /// The row nearest to `time` seconds into `clip`: wrapped for a looping
    /// clip, held at the end for another.
    pub fn frame_at(&self, clip: usize, time: f32) -> usize {
        let meta = &self.clips[clip];
        let f = (time * meta.fps).round() as i64;
        let n = meta.frame_count as i64;
        let local = if meta.looping { f.rem_euclid(n) } else { f.clamp(0, n - 1) };
        meta.first_frame + local as usize
    }

    pub fn contacts(&self, frame: usize) -> Contacts {
        let bits = self.contacts[frame];
        Contacts { left: bits & LEFT_BIT != 0, right: bits & RIGHT_BIT != 0 }
    }

    /// Seconds left in a row's clip; infinite for a looping clip.
    pub fn remaining(&self, frame: usize) -> f32 {
        self.remaining[frame]
    }

    /// Put raw query values (as [`Query::write`] writes them) on the scale the
    /// rows are on.
    pub fn normalize_query(&self, raw: &mut [f32]) {
        for (d, x) in raw.iter_mut().enumerate().take(self.dims()) {
            *x = (*x - self.mean[d]) / self.scale[self.dim_group[d]];
        }
    }

    /// The raw values of a row, as they were before normalization.
    pub fn row_raw(&self, frame: usize, out: &mut Vec<f32>) {
        let dims = self.dims();
        out.clear();
        out.extend(
            self.features[frame * dims..(frame + 1) * dims]
                .iter()
                .enumerate()
                .map(|(d, x)| x * self.scale[self.dim_group[d]] + self.mean[d]),
        );
    }

    /// How far a row is from a normalized query, weighted. Stops adding once
    /// the sum reaches `limit`, since the caller only wants it if it is lower.
    pub fn cost(&self, frame: usize, query: &[f32], limit: f32) -> f32 {
        let dims = self.dims();
        let row = &self.features[frame * dims..(frame + 1) * dims];
        let mut sum = 0.0_f32;
        for d in 0..dims {
            let e = row[d] - query[d];
            sum += self.dim_weight[d] * e * e;
            if sum >= limit {
                return sum;
            }
        }
        sum
    }
}

/// Synthetic clips with known gaits, for the tests of this directory.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use std::f32::consts::PI;

    pub const FPS: f32 = 30.0;

    /// One foot at `phase` of a stride: planted and sliding back through the
    /// root for the first half, lifted and swung forward for the second.
    pub fn foot(phase: f32, half_stride: f32, lift: f32) -> Vec3 {
        let p = phase.rem_euclid(1.0);
        if p < 0.5 {
            Vec3::new(0.0, 0.0, -half_stride + (p / 0.5) * 2.0 * half_stride)
        } else {
            let s = (p - 0.5) / 0.5;
            Vec3::new(0.0, lift * (PI * s).sin(), half_stride - s * 2.0 * half_stride)
        }
    }

    /// The pose of a walker at `phase` of its stride.
    pub fn pose(phase: f32, half_stride: f32, lift: f32) -> PoseFrame {
        let mut left = foot(phase, half_stride, lift);
        let mut right = foot(phase + 0.5, half_stride, lift);
        left.x = -0.1;
        right.x = 0.1;
        PoseFrame { left_foot: left, right_foot: right, hips_height: 1.0 + 0.02 * (4.0 * PI * phase).sin() }
    }

    /// A looping gait of `frames` frames per stride, travelling `speed` m/s.
    pub fn cycle(name: &str, frames: usize, speed: f32, half_stride: f32, lift: f32) -> ClipFrames {
        ClipFrames {
            name: name.to_string(),
            fps: FPS,
            looping: true,
            tags: vec!["locomotion".to_string()],
            frames: (0..frames).map(|i| pose(i as f32 / frames as f32, half_stride, lift)).collect(),
            root: Vec::new(),
        }
        .with_constant_root(speed, 0.0, 0.0)
    }

    pub fn walk() -> ClipFrames {
        cycle("walk", 30, 1.45, 0.36, 0.10)
    }

    pub fn run() -> ClipFrames {
        cycle("run", 20, 3.9, 0.65, 0.20)
    }

    pub fn idle() -> ClipFrames {
        let frame = PoseFrame { left_foot: Vec3::new(-0.1, 0.0, 0.0), right_foot: Vec3::new(0.1, 0.0, 0.0), hips_height: 1.0 };
        ClipFrames {
            name: "idle".to_string(),
            fps: FPS,
            looping: true,
            tags: vec!["locomotion".to_string()],
            frames: vec![frame; 30],
            root: Vec::new(),
        }
        .with_constant_root(0.0, 0.0, 0.0)
    }

    /// A turn in place through 180 degrees in a second, which does not loop.
    pub fn turn180() -> ClipFrames {
        let mut clip = idle();
        clip.name = "turn180".to_string();
        clip.looping = false;
        clip.tags = vec!["turn".to_string()];
        clip.with_constant_root(0.0, 0.0, 180.0)
    }

    pub fn library() -> Database {
        Database::build(
            &[walk(), run(), idle(), turn180()],
            FeatureLayout::new(4),
            &super::super::trajectory::DEFAULT_TRAJECTORY_TIMES,
            FeatureWeights::default(),
        )
        .expect("the synthetic library builds")
    }

    /// The query a character at `frame` of `clip`, moving the way the clip
    /// does, would make, built through the public path (the trajectory from the
    /// predictor, the feet from the pose).
    pub fn query_for(clip: &ClipFrames, frame: usize, db: &Database) -> Query {
        use super::super::trajectory::{predict_trajectory, Desired, PlanarState};
        let n = clip.frames.len();
        let prev = (frame + n - 1) % n;
        let next = (frame + 1) % n;
        let dt = 2.0 / clip.fps;
        let f = clip.frames[frame];
        let root = clip.root[frame];
        // Moving at the clip's own velocity along the way it faces.
        let velocity = to_world(root.velocity, 0.0);
        let mut trajectory = Vec::new();
        predict_trajectory(
            &PlanarState { velocity, ..Default::default() },
            &Desired { velocity, yaw: 0.0 },
            0.15,
            db.times(),
            &mut trajectory,
        );
        Query {
            trajectory,
            left_foot: f.left_foot,
            right_foot: f.right_foot,
            left_foot_velocity: (clip.frames[next].left_foot - clip.frames[prev].left_foot) / dt,
            right_foot_velocity: (clip.frames[next].right_foot - clip.frames[prev].right_foot) / dt,
            root_velocity: Vec3::new(root.velocity.x, (clip.frames[next].hips_height - clip.frames[prev].hips_height) / dt, root.velocity.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::animation::motion::trajectory::DEFAULT_TRAJECTORY_TIMES;

    #[test]
    fn the_library_has_a_row_for_every_frame_and_the_layout_s_width() {
        let db = library();
        assert_eq!(db.dims(), 31);
        assert_eq!(db.frame_count(), 30 + 20 + 30 + 30);
        assert_eq!(db.clip_count(), 4);
        let mut raw = Vec::new();
        db.row_raw(0, &mut raw);
        assert_eq!(raw.len(), 31);
    }

    #[test]
    fn looping_clips_never_run_out_and_others_do() {
        let db = library();
        assert!(db.remaining(0).is_infinite(), "walk loops");
        let turn = db.clips().iter().position(|c| c.name == "turn180").unwrap();
        let first = db.clips()[turn].first_frame;
        assert!((db.remaining(first) - 29.0 / 30.0).abs() < 1.0e-5);
        assert_eq!(db.remaining(first + 29), 0.0);
        assert_eq!(db.frame_info(first + 10).clip, turn);
        assert!((db.frame_info(first + 15).time - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn frames_wrap_in_a_loop_and_hold_at_an_end() {
        let db = library();
        let walk = db.frame_at(0, 0.0);
        assert_eq!(db.frame_at(0, 1.0), walk, "a second later a 1 s loop is back at the start");
        assert_eq!(db.frame_at(0, -1.0 / 30.0), walk + 29, "and before the start it wraps backward");
        let turn = db.clips().iter().position(|c| c.name == "turn180").unwrap();
        let end = db.clips()[turn].first_frame + 29;
        assert_eq!(db.frame_at(turn, 5.0), end, "a clip that does not loop holds its last frame");
    }

    /// The planted foot of an in-place walk slides back at ground speed; only
    /// the speed against the ground says it is planted.
    #[test]
    fn contacts_follow_the_planted_foot() {
        let db = library();
        let left = (0..30).filter(|&f| db.contacts(f).left).count();
        let right = (0..30).filter(|&f| db.contacts(f).right).count();
        assert!((12..=18).contains(&left), "left planted for about half the stride, got {left}");
        assert!((12..=18).contains(&right), "right planted for about half the stride, got {right}");
        let neither = (0..30).filter(|&f| !db.contacts(f).left && !db.contacts(f).right).count();
        assert!(neither <= 3, "one foot is nearly always down when walking, {neither} frames have none");
        let both = (0..30).filter(|&f| db.contacts(f).left && db.contacts(f).right).count();
        assert!(both <= 3, "the feet take turns, {both} frames have both");
        // At rest both feet are down all the time.
        let idle = db.clips().iter().find(|c| c.name == "idle").unwrap().first_frame;
        assert!((0..30).all(|f| db.contacts(idle + f).left && db.contacts(idle + f).right));
    }

    #[test]
    fn a_walk_s_stored_path_is_the_path_the_predictor_draws() {
        let db = library();
        let walk = walk();
        let query = query_for(&walk, 9, &db);
        let mut raw = Vec::new();
        db.row_raw(9, &mut raw);
        let layout = db.layout();
        let mut ours = Vec::new();
        assert!(query.write(layout, &mut ours));
        for group in [Group::TrajectoryPosition, Group::TrajectoryFacing] {
            for d in layout.range(group) {
                assert!((raw[d] - ours[d]).abs() < 2.0e-3, "{group:?} value {d}: stored {} drawn {}", raw[d], ours[d]);
            }
        }
        // 1.45 m/s straight ahead: 0.29 m at 0.2 s.
        assert!((raw[1] + 0.29).abs() < 1.0e-3, "got {}", raw[1]);
    }

    #[test]
    fn a_turning_clip_stores_a_turning_path() {
        let db = library();
        let turn = db.clips().iter().find(|c| c.name == "turn180").unwrap().first_frame;
        let mut raw = Vec::new();
        db.row_raw(turn, &mut raw);
        let layout = db.layout();
        let facing = &raw[layout.range(Group::TrajectoryFacing)];
        // 180 degrees a second, left: at 0.2 s it faces 36 degrees to the left,
        // which is local (-sin 36, -cos 36); at 1.0 s it faces backward.
        let a = 36.0_f32.to_radians();
        assert!((facing[0] + a.sin()).abs() < 1.0e-3 && (facing[1] + a.cos()).abs() < 1.0e-3, "{facing:?}");
        assert!(facing[6].abs() < 1.0e-3 && (facing[7] - 1.0).abs() < 1.0e-3, "backward by 1 s: {facing:?}");
    }

    #[test]
    fn a_constant_root_reads_its_attributes() {
        let clip = walk().with_constant_root(2.0, 90.0, 0.0);
        assert!((clip.root[0].velocity - Vec2::new(2.0, 0.0)).length() < 1.0e-5, "90 degrees is the right");
        let clip = walk().with_constant_root(2.0, 180.0, 90.0);
        assert!((clip.root[0].velocity - Vec2::new(0.0, 2.0)).length() < 1.0e-5, "180 degrees is backward");
        assert!((clip.root[0].yaw_rate - std::f32::consts::FRAC_PI_2).abs() < 1.0e-5);
    }

    #[test]
    fn building_twice_gives_the_same_database() {
        assert_eq!(library(), library());
    }

    #[test]
    fn tags_get_bits_in_the_order_they_appear() {
        let db = library();
        assert_eq!(db.tag_names(), ["locomotion".to_string(), "turn".to_string()]);
        assert_eq!(db.tag_bit("locomotion"), Some(1));
        assert_eq!(db.tag_bit(" turn "), Some(2));
        assert_eq!(db.tag_bit("vault"), None);
        let turn = db.clips().iter().find(|c| c.name == "turn180").unwrap();
        assert_eq!(turn.tag_bits, 2);
    }

    #[test]
    fn input_that_cannot_be_matched_is_refused() {
        let layout = FeatureLayout::new(4);
        let times = DEFAULT_TRAJECTORY_TIMES;
        let weights = FeatureWeights::default();
        assert!(Database::build(&[], layout, &times, weights).is_err(), "no clips");
        let mut one = walk();
        one.frames.truncate(1);
        one.root.truncate(1);
        assert!(Database::build(&[one], layout, &times, weights).is_err(), "one frame");
        let mut short = walk();
        short.root.pop();
        assert!(Database::build(&[short], layout, &times, weights).is_err(), "root motion short of frames");
        let mut still = walk();
        still.fps = 0.0;
        assert!(Database::build(&[still], layout, &times, weights).is_err(), "no frame rate");
        let mut nan = walk();
        nan.frames[3].hips_height = f32::NAN;
        assert!(Database::build(&[nan], layout, &times, weights).is_err(), "not a number");
        assert!(Database::build(&[walk()], layout, &times[..3], weights).is_err(), "times against the layout");
        assert!(Database::build(&[walk()], layout, &[0.4, 0.2, 0.6, 1.0], weights).is_err(), "descending times");
    }

    #[test]
    fn a_row_is_nearest_to_itself_and_weights_change_the_cost_without_a_rebuild() {
        let mut db = library();
        let mut q = Vec::new();
        db.row_raw(7, &mut q);
        db.normalize_query(&mut q);
        assert!(db.cost(7, &q, f32::INFINITY) < 1.0e-6, "a row costs nothing against itself");
        let far = db.cost(70, &q, f32::INFINITY);
        assert!(far > 0.0);
        let clipped = db.cost(70, &q, 0.0001);
        assert!(clipped >= 0.0001 && clipped <= far, "the sum stops once it reaches the limit");
        db.set_weights(FeatureWeights::from_class(0.0, 0.0, 0.0, 0.0));
        assert_eq!(db.cost(70, &q, f32::INFINITY), 0.0, "weights of zero make every row equal");
    }
}

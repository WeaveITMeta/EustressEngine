//! # Search
//!
//! The matcher plays one clip at a time and, now and then, looks through the
//! whole database for a row that fits the character better. It scans every
//! row: at ten thousand rows, ten searches a second and two hundred characters
//! that is a few million multiply-adds a frame, so a search tree would cost more
//! than it saves.
//!
//! A search runs when the interval has passed, when the clip being played has
//! ended, when there is no clip yet, or when the caller says it is urgent
//! (the character left or reached the ground, or its desired speed or heading
//! changed a lot). To stop it thrashing between clips that fit about equally:
//!
//! - the cost of carrying on with the current clip is multiplied by
//!   `1 - hysteresis`, so a candidate has to clearly win;
//! - nothing switches inside `min_dwell` of the last switch, unless urgent;
//! - a match in the same clip within `same_clip_window` of where it already is
//!   is ignored, since that is only the clip playing on;
//! - rows within `blend_time` of the end of a clip that does not loop are not
//!   candidates, since the clip would end before the blend finished.
//!
//! A clip that does not loop and reaches its end is always left.

use super::database::Database;
use super::features::Query;

/// How a [`Matcher`] decides to switch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchConfig {
    /// Seconds between searches.
    pub interval: f32,
    /// Seconds a switch takes to settle; rows closer than this to the end of a
    /// clip that does not loop are skipped.
    pub blend_time: f32,
    /// Seconds a clip plays before another may replace it.
    pub min_dwell: f32,
    /// The fraction a candidate's cost must be under the current clip's.
    pub hysteresis: f32,
    /// A match this close in time, in the clip already playing, is ignored.
    pub same_clip_window: f32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self { interval: 0.1, blend_time: 0.2, min_dwell: 0.15, hysteresis: 0.1, same_clip_window: 0.25 }
    }
}

/// Which clips a search may use, by tag bits (see [`Database::tag_bit`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagFilter {
    /// A clip must carry every one of these tags.
    pub require: u64,
    /// A clip must carry none of these.
    pub exclude: u64,
}

impl TagFilter {
    pub fn allows(&self, tag_bits: u64) -> bool {
        tag_bits & self.require == self.require && tag_bits & self.exclude == 0
    }
}

/// Where the matcher is after a step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchStep {
    pub clip: usize,
    /// Seconds into the clip.
    pub time: f32,
    /// The row nearest to `time`.
    pub frame: usize,
    /// How well the current clip fits (the last search's cost).
    pub cost: f32,
    /// Whether this step ran a search.
    pub searched: bool,
    /// Whether it moved to a different place: the caller starts an inertialized
    /// transition when it did.
    pub switched: bool,
}

/// Plays one clip of a database and searches it for a better one.
#[derive(Debug, Clone)]
pub struct Matcher {
    config: SearchConfig,
    clip: Option<usize>,
    time: f32,
    since_switch: f32,
    since_search: f32,
    cost: f32,
    ended: bool,
    /// Where in the interval this matcher's searches fall, 0 to 1.
    phase: f32,
    searches: u32,
    scratch: Vec<f32>,
}

impl Matcher {
    pub fn new(config: SearchConfig) -> Self {
        Self {
            config,
            clip: None,
            time: 0.0,
            since_switch: 0.0,
            since_search: 0.0,
            cost: f32::INFINITY,
            ended: false,
            phase: 0.0,
            searches: 0,
            scratch: Vec::new(),
        }
    }

    /// Put this matcher's searches `phase` (0 to 1) of the way through the
    /// interval, so a crowd of characters does not search on the same frame.
    /// The first search happens at once, since nothing is playing; the phase
    /// shows from the next one on.
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase.rem_euclid(1.0);
        self
    }

    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: SearchConfig) {
        self.config = config;
    }

    /// The clip playing and the seconds into it.
    pub fn current(&self) -> Option<(usize, f32)> {
        self.clip.map(|c| (c, self.time))
    }

    /// Forget the clip, so the next step searches from scratch.
    pub fn reset(&mut self) {
        self.clip = None;
        self.ended = false;
        self.cost = f32::INFINITY;
    }

    /// Play on by `dt` seconds, and search if it is time to.
    ///
    /// `query` is the character's pose and predicted path now. `urgent` asks for
    /// a search now that may also switch inside the dwell time. `None` when the
    /// database has no clip the filter allows and nothing is playing.
    pub fn step(
        &mut self,
        db: &Database,
        dt: f32,
        query: &Query,
        filter: TagFilter,
        urgent: bool,
    ) -> Option<MatchStep> {
        self.advance(db, if dt > 0.0 { dt } else { 0.0 });
        let due = self.clip.is_none() || self.ended || urgent || self.since_search >= self.config.interval;
        let (mut searched, mut switched) = (false, false);
        if due {
            let mut raw = std::mem::take(&mut self.scratch);
            if query.write(db.layout(), &mut raw) {
                db.normalize_query(&mut raw);
                (searched, switched) = self.search(db, &raw, filter, urgent);
            }
            self.scratch = raw;
        }
        let clip = self.clip?;
        Some(MatchStep { clip, time: self.time, frame: db.frame_at(clip, self.time), cost: self.cost, searched, switched })
    }

    fn advance(&mut self, db: &Database, dt: f32) {
        if let Some(clip) = self.clip {
            let meta = &db.clips()[clip];
            self.time += dt;
            if meta.looping {
                self.time = self.time.rem_euclid(meta.duration.max(1.0e-6));
            } else if self.time >= meta.duration {
                self.time = meta.duration;
                self.ended = true;
            }
        }
        self.since_switch += dt;
        self.since_search += dt;
    }

    /// Scan the rows. Returns whether it searched and whether it switched.
    fn search(&mut self, db: &Database, query: &[f32], filter: TagFilter, urgent: bool) -> (bool, bool) {
        let mut best: Option<(usize, f32)> = None;
        let mut limit = f32::INFINITY;
        for meta in db.clips() {
            if !filter.allows(meta.tag_bits) {
                continue;
            }
            for local in 0..meta.frame_count {
                let frame = meta.first_frame + local;
                if db.remaining(frame) < self.config.blend_time {
                    continue;
                }
                let cost = db.cost(frame, query, limit);
                if cost < limit {
                    limit = cost;
                    best = Some((frame, cost));
                }
            }
        }
        self.since_search = if self.searches == 0 { self.phase * self.config.interval } else { 0.0 };
        self.searches = self.searches.saturating_add(1);

        let continuing = match self.clip {
            Some(clip) if !self.ended && filter.allows(db.clips()[clip].tag_bits) => {
                db.cost(db.frame_at(clip, self.time), query, f32::INFINITY)
            }
            _ => f32::INFINITY,
        };
        let Some((frame, cost)) = best else {
            return (true, false);
        };
        let info = db.frame_info(frame);
        let take = match self.clip {
            None => true,
            Some(_) if !continuing.is_finite() => true,
            Some(clip) => {
                let meta = &db.clips()[clip];
                let gap = (info.time - self.time).abs();
                let gap = if meta.looping { gap.min((meta.duration - gap).abs()) } else { gap };
                let same_place = info.clip == clip && gap < self.config.same_clip_window;
                let clearly_better = cost < continuing * (1.0 - self.config.hysteresis);
                let dwelled = urgent || self.since_switch >= self.config.min_dwell;
                clearly_better && dwelled && !same_place
            }
        };
        if take {
            self.clip = Some(info.clip);
            self.time = info.time;
            self.since_switch = 0.0;
            self.ended = false;
            self.cost = cost;
        } else {
            self.cost = continuing;
        }
        (true, take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::motion::database::fixtures::*;
    use crate::animation::motion::trajectory::{forward, TrajectoryPoint, DEFAULT_TRAJECTORY_TIMES};
    use bevy::math::{Vec2, Vec3};
    use std::f32::consts::PI;

    const WALK: usize = 0;
    const RUN: usize = 1;
    const IDLE: usize = 2;
    const TURN: usize = 3;

    fn first_step(db: &Database, query: &Query, filter: TagFilter) -> MatchStep {
        Matcher::new(SearchConfig::default()).step(db, 0.0, query, filter, false).expect("a match")
    }

    /// A character standing still and turning left at 180 degrees a second.
    fn turning_query() -> Query {
        Query {
            trajectory: DEFAULT_TRAJECTORY_TIMES
                .iter()
                .map(|t| TrajectoryPoint { position: Vec2::ZERO, facing: forward(PI * t) })
                .collect(),
            left_foot: Vec3::new(-0.1, 0.0, 0.0),
            right_foot: Vec3::new(0.1, 0.0, 0.0),
            ..Default::default()
        }
    }

    #[test]
    fn a_walking_character_matches_the_walk_at_its_own_phase() {
        let db = library();
        let walk = walk();
        for frame in [0, 5, 9, 14, 20, 27] {
            let step = first_step(&db, &query_for(&walk, frame, &db), TagFilter::default());
            assert_eq!(step.clip, WALK, "frame {frame}");
            let gap = (step.time - frame as f32 / FPS).abs();
            let gap = gap.min(1.0 - gap);
            assert!(gap < 1.5 / FPS, "frame {frame} matched at {}", step.time);
            assert!(step.searched && step.switched, "the first step picks a clip");
        }
    }

    #[test]
    fn a_run_matches_the_run_and_standing_matches_the_idle() {
        let db = library();
        assert_eq!(first_step(&db, &query_for(&run(), 7, &db), TagFilter::default()).clip, RUN);
        assert_eq!(first_step(&db, &query_for(&idle(), 3, &db), TagFilter::default()).clip, IDLE);
    }

    #[test]
    fn a_path_that_turns_a_half_circle_picks_the_turn() {
        let db = library();
        let step = first_step(&db, &turning_query(), TagFilter::default());
        assert_eq!(step.clip, TURN);
        assert!(step.time < 0.1, "at the start of the turn, got {}", step.time);
    }

    #[test]
    fn tags_narrow_the_search() {
        let db = library();
        let turn = TagFilter { require: db.tag_bit("turn").unwrap(), exclude: 0 };
        assert_eq!(first_step(&db, &query_for(&walk(), 5, &db), turn).clip, TURN, "only the turn is allowed");
        let no_turn = TagFilter { require: 0, exclude: db.tag_bit("turn").unwrap() };
        assert_ne!(first_step(&db, &turning_query(), no_turn).clip, TURN);
        let nothing = TagFilter { require: 1 << 40, exclude: 0 };
        assert!(Matcher::new(SearchConfig::default()).step(&db, 0.0, &turning_query(), nothing, false).is_none());
    }

    /// Playing along a clip that keeps fitting never switches.
    #[test]
    fn carrying_on_is_not_a_switch() {
        let db = library();
        let walk = walk();
        let mut matcher = Matcher::new(SearchConfig::default());
        let first = matcher.step(&db, 0.0, &query_for(&walk, 5, &db), TagFilter::default(), false).unwrap();
        assert!(first.switched);
        let mut last = first;
        for k in 1..=20 {
            let frame = (5 + k) % 30;
            last = matcher.step(&db, 1.0 / FPS, &query_for(&walk, frame, &db), TagFilter::default(), false).unwrap();
            assert!(!last.switched, "step {k} switched to clip {} at {}", last.clip, last.time);
            assert_eq!(last.clip, WALK);
        }
        assert!((last.time - 25.0 / FPS).abs() < 1.0e-3, "time moved on with the steps, got {}", last.time);
    }

    #[test]
    fn the_dwell_holds_a_switch_back_unless_it_is_urgent() {
        let db = library();
        let config = SearchConfig { min_dwell: 0.45, ..Default::default() };
        let walking = query_for(&walk(), 5, &db);
        let running = query_for(&run(), 7, &db);

        let mut matcher = Matcher::new(config);
        assert_eq!(matcher.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap().clip, WALK);
        for k in 1..=4 {
            let step = matcher.step(&db, 0.1, &running, TagFilter::default(), false).unwrap();
            assert!(step.searched && !step.switched, "step {k} is inside the dwell");
            assert_eq!(step.clip, WALK);
        }
        let step = matcher.step(&db, 0.1, &running, TagFilter::default(), false).unwrap();
        assert!(step.switched && step.clip == RUN, "the dwell is over");

        let mut matcher = Matcher::new(config);
        matcher.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        let step = matcher.step(&db, 0.02, &running, TagFilter::default(), true).unwrap();
        assert!(step.switched && step.clip == RUN, "urgent skips the dwell");
    }

    #[test]
    fn hysteresis_decides_how_clearly_a_candidate_must_win() {
        let db = library();
        let walking = query_for(&walk(), 5, &db);
        let running = query_for(&run(), 7, &db);
        let stubborn = SearchConfig { min_dwell: 0.0, hysteresis: 1.0, ..Default::default() };
        let mut matcher = Matcher::new(stubborn);
        matcher.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        for _ in 0..5 {
            let step = matcher.step(&db, 0.1, &running, TagFilter::default(), false).unwrap();
            assert!(!step.switched && step.clip == WALK, "a candidate can never win by 100 percent");
        }
        let eager = SearchConfig { min_dwell: 0.0, hysteresis: 0.0, ..Default::default() };
        let mut matcher = Matcher::new(eager);
        matcher.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        let step = matcher.step(&db, 0.1, &running, TagFilter::default(), false).unwrap();
        assert!(step.switched && step.clip == RUN);
    }

    #[test]
    fn a_clip_that_ends_is_left() {
        let db = library();
        let mut matcher = Matcher::new(SearchConfig::default());
        let turn = TagFilter { require: db.tag_bit("turn").unwrap(), exclude: 0 };
        let first = matcher.step(&db, 0.0, &query_for(&walk(), 5, &db), turn, false).unwrap();
        assert_eq!(first.clip, TURN);
        let step = matcher.step(&db, 2.0, &query_for(&walk(), 5, &db), TagFilter::default(), false).unwrap();
        assert!(step.switched && step.clip == WALK, "a turn that has run out gives way to the walk");
    }

    #[test]
    fn a_query_that_cannot_be_written_does_not_search() {
        let db = library();
        let mut matcher = Matcher::new(SearchConfig::default());
        let broken = Query::default();
        assert!(matcher.step(&db, 0.0, &broken, TagFilter::default(), false).is_none(), "nothing playing, nothing to go on");
        matcher.step(&db, 0.0, &query_for(&walk(), 5, &db), TagFilter::default(), false).unwrap();
        let step = matcher.step(&db, 0.5, &broken, TagFilter::default(), true).unwrap();
        assert!(!step.searched && !step.switched && step.clip == WALK);
        assert!((step.time - (5.0 / FPS + 0.5)).abs() < 1.0e-3, "the clip played on, got {}", step.time);
    }

    #[test]
    fn a_phase_staggers_the_first_scheduled_search() {
        let db = library();
        let walking = query_for(&walk(), 5, &db);
        let mut a = Matcher::new(SearchConfig::default());
        let mut b = Matcher::new(SearchConfig::default()).with_phase(0.5);
        a.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        b.step(&db, 0.0, &walking, TagFilter::default(), false).unwrap();
        // 0.06 s on: the one that started half an interval in is due, the other is not.
        assert!(!a.step(&db, 0.06, &walking, TagFilter::default(), false).unwrap().searched);
        assert!(b.step(&db, 0.06, &walking, TagFilter::default(), false).unwrap().searched);
    }
}

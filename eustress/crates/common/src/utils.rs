//! Shared utility functions

/// Generate a unique name with numeric suffix
pub fn unique_name(base: &str, existing: &[String]) -> String {
    let mut counter = 1;
    loop {
        let name = if counter == 1 {
            base.to_string()
        } else {
            format!("{} {}", base, counter)
        };
        
        if !existing.contains(&name) {
            return name;
        }
        counter += 1;
    }
}

// ── Performance switches ────────────────────────────────────────────────
//
// A performance change that keeps an equivalent older path keeps both behind
// a named switch, so one binary can be measured with the change on and off:
// alternate launches on the same Space, same camera. Every switch is on
// unless `EUSTRESS_PERF_OFF` names it (a comma-separated list, or `all`). The
// variable is read once, at first use, and does not change during a run, so
// a switch may decide which systems a plugin registers.

/// The switches `EUSTRESS_PERF_OFF` turns off, lower-cased.
pub fn perf_switches_off() -> &'static [String] {
    static OFF: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    OFF.get_or_init(|| {
        std::env::var("EUSTRESS_PERF_OFF")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

/// Whether the performance change named `key` is on (see above).
pub fn perf_on(key: &str) -> bool {
    !perf_switches_off()
        .iter()
        .any(|s| s == "all" || s.eq_ignore_ascii_case(key))
}

/// Whether `query` matches at least one entity, found with a parallel walk.
///
/// `Query::is_empty` stops at the first match, but on a quiet frame nothing
/// matches, so a change-filtered query checks every row on one thread. This
/// checks the same rows spread over the compute pool. The answer is the same
/// and so is the change window, which belongs to the calling system.
pub fn par_any<F: bevy::ecs::query::QueryFilter>(query: &bevy::prelude::Query<(), F>) -> bool {
    // `par_iter` panics when no compute pool exists (a bare `World` in a
    // test); the serial check gives the same answer there.
    if bevy::tasks::ComputeTaskPool::try_get().is_none() {
        return !query.is_empty();
    }
    let hit = std::sync::atomic::AtomicBool::new(false);
    query.par_iter().for_each(|()| {
        hit.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    hit.into_inner()
}

/// Whether a change probe matches anything: [`par_any`] under the
/// `idle_par_any` switch, `!is_empty()` without it.
pub fn probe_any<F: bevy::ecs::query::QueryFilter>(query: &bevy::prelude::Query<(), F>) -> bool {
    if perf_on("idle_par_any") {
        par_any(query)
    } else {
        !query.is_empty()
    }
}

/// How many entities `query` matches, summed over its tables instead of
/// counted one by one. Only filters decided per archetype (`With`,
/// `Without`) are accepted, which is what makes the iterator's size hint
/// exact.
pub fn count_matching<F: bevy::ecs::query::ArchetypeFilter>(
    query: &bevy::prelude::Query<(), F>,
) -> usize {
    query.iter().size_hint().0
}

/// Run condition: true when a `T` may have been added since this condition
/// last ran, so a system that walks `Added<T>` can skip quiet frames.
///
/// The number of entities with `T` is summed over tables, one pass over the
/// tables instead of a tick check on every entity. An add raises that number
/// unless a `T` also went away in the same interval, and every removal
/// (despawns included) shows up in `RemovedComponents<T>`, so no add is
/// missed. A skipped system keeps its last-run tick, so its next real run
/// sees every `T` added since it last ran, in the frame it would have.
/// Always true with the `added_counts` performance switch off.
pub fn maybe_added<T: bevy::prelude::Component>(
    live: bevy::prelude::Query<(), bevy::prelude::With<T>>,
    mut removed: bevy::prelude::RemovedComponents<T>,
    mut last_count: bevy::prelude::Local<Option<usize>>,
) -> bool {
    if !perf_on("added_counts") {
        return true;
    }
    let count = count_matching(&live);
    let any_removed = !removed.is_empty();
    removed.clear();
    let changed = any_removed || *last_count != Some(count);
    *last_count = Some(count);
    changed
}

// ── Frame staggering ────────────────────────────────────────────────────
//
// A system whose work follows change ticks or current state can run on one
// frame in `STAGGER_PERIOD` instead of on every frame. Each such system has a
// slot in `stagger_slots`, and systems on different slots never share a
// frame, so their cost spreads over the cycle instead of landing on every
// frame. Switch: `frame_stagger`.

/// Frames in one stagger cycle.
pub const STAGGER_PERIOD: u32 = 4;

/// The frame of the cycle each staggered system runs on, the frames where
/// `FrameCount % period == slot`. The balance is set here and nowhere else.
/// A system gets an entry only once it meets the rules on `stagger` and its
/// `stagger_check` test passes; a system that carries Play animation to the
/// screen or to physics (GUI, lights, materials, colliders, billboards, mass),
/// that follows the camera, or that streams every change never gets one.
pub mod stagger_slots {
    /// `portal::sync_portal_markers`.
    pub const SYNC_PORTAL_MARKERS: u32 = 3;
}

/// Forces frame staggering on or off for one `App`, whatever
/// `EUSTRESS_PERF_OFF` says and whether a load is running. Tests insert it to
/// run the same inputs both ways.
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameStaggerOverride(pub bool);

static STAGGER_HOLD: std::sync::OnceLock<fn() -> bool> = std::sync::OnceLock::new();

/// Registers the probe that says a bulk load is running. While it returns
/// true every slot is open: the load throttle already spaces the UI syncs
/// out then, and a fixed slot could fall between its frames for the whole
/// load. The first registration wins.
pub fn set_stagger_hold(probe: fn() -> bool) {
    let _ = STAGGER_HOLD.set(probe);
}

/// Whether a system on `slot` runs this frame.
fn stagger_open(period: u32, slot: u32, frame: u32, forced: Option<bool>) -> bool {
    match forced {
        Some(false) => return true,
        Some(true) => {}
        None => {
            if !perf_on("frame_stagger") || STAGGER_HOLD.get().is_some_and(|hold| hold()) {
                return true;
            }
        }
    }
    frame % period == slot
}

/// Run condition: true on one frame in `period`, the frames where
/// `FrameCount % period == slot`, so systems on different slots never share
/// a frame's cost. Take the slot from [`stagger_slots`].
///
/// Only for a system whose work follows change ticks (`Changed`, `Added`,
/// `Ref::is_changed`) or current state: a skipped run keeps its last-run
/// tick, so the next run sees everything since, up to `period - 1` frames
/// late and none of it lost. Never for a system that reads
/// `RemovedComponents` or messages (Bevy keeps those for about one frame),
/// that feeds input or rendering, or that moves something the user watches.
///
/// Open on every frame while a bulk load runs (see [`set_stagger_hold`]) and
/// when `EUSTRESS_PERF_OFF` names `frame_stagger`. Bevy evaluates every run
/// condition of a system on every frame, so a stateful condition beside this
/// one still updates on the skipped frames; to combine one with a slot, see
/// [`maybe_added_staggered`].
pub fn stagger(
    period: u32,
    slot: u32,
) -> impl FnMut(
    bevy::prelude::Res<bevy::diagnostic::FrameCount>,
    Option<bevy::prelude::Res<FrameStaggerOverride>>,
) -> bool
       + Clone {
    assert!(slot < period, "stagger slot {slot} is outside period {period}");
    move |frame: bevy::prelude::Res<bevy::diagnostic::FrameCount>,
          forced: Option<bevy::prelude::Res<FrameStaggerOverride>>| {
        stagger_open(period, slot, frame.0, forced.map(|f| f.0))
    }
}

/// [`maybe_added`] on a stagger slot, as one condition. The add check runs on
/// every frame and is remembered until the slot comes round, so an add seen
/// on a skipped frame still runs the system on its next slot. Stacking
/// `run_if(maybe_added)` beside `run_if(stagger(..))` would lose it: the add
/// check reports each add once, on the frame it sees it.
pub fn maybe_added_staggered<T: bevy::prelude::Component>(
    period: u32,
    slot: u32,
) -> impl FnMut(
    bevy::prelude::Query<(), bevy::prelude::With<T>>,
    bevy::prelude::RemovedComponents<T>,
    bevy::prelude::Local<(Option<usize>, bool)>,
    bevy::prelude::Res<bevy::diagnostic::FrameCount>,
    Option<bevy::prelude::Res<FrameStaggerOverride>>,
) -> bool
       + Clone {
    assert!(slot < period, "stagger slot {slot} is outside period {period}");
    move |live: bevy::prelude::Query<(), bevy::prelude::With<T>>,
          mut removed: bevy::prelude::RemovedComponents<T>,
          mut state: bevy::prelude::Local<(Option<usize>, bool)>,
          frame: bevy::prelude::Res<bevy::diagnostic::FrameCount>,
          forced: Option<bevy::prelude::Res<FrameStaggerOverride>>| {
        let (last_count, pending) = &mut *state;
        let any_removed = !removed.is_empty();
        removed.clear();
        if perf_on("added_counts") {
            let count = count_matching(&live);
            *pending |= any_removed || *last_count != Some(count);
            *last_count = Some(count);
        } else {
            *pending = true;
        }
        if !stagger_open(period, slot, frame.0, forced.map(|f| f.0)) {
            return false;
        }
        std::mem::take(pending)
    }
}

/// Test support for [`stagger`]: proof that a slot delays a system's work and
/// loses none of it.
#[doc(hidden)]
pub mod stagger_check {
    use bevy::prelude::{App, World};

    /// Runs the same inputs through two `App`s, one with staggering forced on
    /// and one forced off, then gives both `2 * STAGGER_PERIOD` frames with no
    /// input, and checks that:
    /// - on every frame the staggered output equals the every-frame output of
    ///   that frame or of one of the `STAGGER_PERIOD - 1` frames before it,
    ///   counting the output before the first frame (late, never wrong, never
    ///   lost),
    /// - the two outputs end the same, and
    /// - the staggered output lagged at least once, so the slot was in force
    ///   and the checks above could have failed.
    ///
    /// `build` registers the system under its production slot condition,
    /// plus anything that reads its output. `input(world, frame)` changes the
    /// world before frame `frame` runs. `output` reads what the system left.
    pub fn assert_same_after_settle<T: PartialEq + std::fmt::Debug>(
        build: impl Fn(&mut App),
        input: impl Fn(&mut World, u32),
        frames: u32,
        output: impl Fn(&mut World) -> T,
    ) {
        let lag = super::STAGGER_PERIOD as usize - 1;
        let run = |staggered: bool| -> Vec<T> {
            let mut app = App::new();
            app.insert_resource(super::FrameStaggerOverride(staggered));
            app.init_resource::<bevy::diagnostic::FrameCount>();
            build(&mut app);
            // Index 0 is the output before any frame; frame `f` is index `f + 1`.
            let mut seen = vec![output(app.world_mut())];
            for frame in 0..frames + 2 * super::STAGGER_PERIOD {
                if frame < frames {
                    input(app.world_mut(), frame);
                }
                app.update();
                seen.push(output(app.world_mut()));
                // Bevy's own counter advances in `Last`, after the frame's systems.
                app.world_mut().resource_mut::<bevy::diagnostic::FrameCount>().0 += 1;
            }
            seen
        };
        let on = run(true);
        let off = run(false);
        for (i, got) in on.iter().enumerate().skip(1) {
            let window = &off[i.saturating_sub(lag)..=i];
            assert!(
                window.contains(got),
                "frame {}: the staggered output {got:?} is none of the every-frame outputs it may lag behind: {window:?}",
                i - 1,
            );
        }
        assert_eq!(on.last(), off.last(), "the outputs differ after settling");
        assert!(
            on.iter().zip(&off).any(|(a, b)| a != b),
            "the staggered output never lagged, so these inputs do not exercise the slot",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_name() {
        let existing = vec!["Cube".to_string(), "Cube 2".to_string()];
        assert_eq!(unique_name("Cube", &existing), "Cube 3");
        assert_eq!(unique_name("Sphere", &existing), "Sphere");
    }

    // ── Frame staggering ──

    use bevy::prelude::*;

    #[derive(Component)]
    struct Score(i32);

    #[derive(Resource, Default)]
    struct Copied(std::collections::BTreeMap<Entity, i32>);

    fn copy_changed(changed: Query<(Entity, &Score), Changed<Score>>, mut out: ResMut<Copied>) {
        for (entity, score) in &changed {
            out.0.insert(entity, score.0);
        }
    }

    fn score_inputs(world: &mut World, frame: u32) {
        match frame {
            0 => {
                world.spawn(Score(1));
                world.spawn(Score(2));
            }
            2 | 5 | 6 => {
                let mut scores = world.query::<&mut Score>();
                if let Some(mut score) = scores.iter_mut(world).next() {
                    score.0 += 10;
                }
            }
            9 => {
                world.spawn(Score(3));
            }
            _ => {}
        }
    }

    #[test]
    fn stagger_opens_one_frame_in_the_period() {
        let open: Vec<u32> = (0..12).filter(|&f| stagger_open(4, 2, f, Some(true))).collect();
        assert_eq!(open, vec![2, 6, 10]);
        assert!((0..12).all(|f| stagger_open(4, 2, f, Some(false))));
    }

    #[test]
    #[should_panic(expected = "outside period")]
    fn a_slot_outside_its_period_is_refused() {
        let _ = stagger(4, 4);
    }

    #[test]
    fn a_change_driven_system_loses_nothing_on_a_slot() {
        stagger_check::assert_same_after_settle(
            |app| {
                app.init_resource::<Copied>()
                    .add_systems(Update, copy_changed.run_if(stagger(STAGGER_PERIOD, 1)));
            },
            score_inputs,
            12,
            |world| world.resource::<Copied>().0.clone(),
        );
    }

    #[derive(Message)]
    struct Ping;

    #[derive(Resource, Default)]
    struct Pings(usize);

    fn count_pings(mut pings: MessageReader<Ping>, mut seen: ResMut<Pings>) {
        seen.0 += pings.read().count();
    }

    /// Bevy keeps a message for about one frame, so a reader on a slot misses
    /// most of them, and the check must say so.
    #[test]
    #[should_panic(expected = "is none of the every-frame outputs")]
    fn the_check_catches_a_message_reader_on_a_slot() {
        stagger_check::assert_same_after_settle(
            |app| {
                app.add_message::<Ping>()
                    .init_resource::<Pings>()
                    .add_systems(Update, count_pings.run_if(stagger(STAGGER_PERIOD, 1)));
            },
            |world, frame| {
                if frame < 3 {
                    world.write_message(Ping);
                }
            },
            8,
            |world| world.resource::<Pings>().0,
        );
    }

    #[derive(Resource, Default)]
    struct AddedSeen(usize);

    fn count_added(added: Query<(), Added<Score>>, mut seen: ResMut<AddedSeen>) {
        seen.0 += added.iter().count();
    }

    /// An add seen on a skipped frame still runs the system on its slot,
    /// including an add that a removal in the same frame hides from the count.
    #[test]
    fn maybe_added_staggered_keeps_an_add_until_its_slot() {
        stagger_check::assert_same_after_settle(
            |app| {
                app.init_resource::<AddedSeen>().add_systems(
                    Update,
                    count_added.run_if(maybe_added_staggered::<Score>(STAGGER_PERIOD, 1)),
                );
            },
            |world, frame| match frame {
                2 => {
                    world.spawn(Score(1));
                }
                6 => {
                    let mut scores = world.query_filtered::<Entity, With<Score>>();
                    let gone = scores.iter(world).next().unwrap();
                    world.despawn(gone);
                    world.spawn(Score(2));
                }
                _ => {}
            },
            10,
            |world| world.resource::<AddedSeen>().0,
        );
    }
}

// ── Script profiling ────────────────────────────────────────────────────
//
// Per-script, per-step timing of the scripting runtimes, to find which script
// a slow frame spent its time in. `EUSTRESS_SCRIPT_PROFILE` arms it (read
// once). The runtimes report each step a script runs (`record`) and all of
// their own work in a frame (`record_driver`), and the DataModel counts the
// reads and writes made inside a `scope`. The engine takes the tally once a
// frame (`take_frame`). Unarmed, every call returns before doing anything
// else and allocates nothing.

/// Per-script, per-step timing of the scripting runtimes (see above).
pub mod script_profile {
    use std::borrow::Cow;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
    use std::time::Duration;

    /// The most characters a script or step name keeps; the rest is cut off.
    pub const MAX_NAME_CHARS: usize = 96;

    /// One step a script ran, as [`capture`] saw it, its names sanitized.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Record {
        pub runtime: &'static str,
        pub script: String,
        pub step: String,
        pub elapsed: Duration,
    }

    /// What one [`capture`] saw on its thread.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct Captured {
        pub records: Vec<Record>,
        pub drivers: Vec<(&'static str, Duration)>,
        pub dm_reads: u64,
        pub dm_writes: u64,
    }

    /// One step over a frame: its summed time and how many calls made it up.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct StepTotal {
        pub runtime: &'static str,
        pub script: Arc<str>,
        pub step: Arc<str>,
        pub total: Duration,
        pub calls: u64,
    }

    /// One frame of script work, from [`take_frame`]. Names are sanitized.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct FrameTally {
        /// Every step that ran in the frame, in no set order.
        pub steps: Vec<StepTotal>,
        /// Each runtime's whole driver time in the frame.
        pub drivers: Vec<(&'static str, Duration)>,
        /// DataModel reads and writes made inside a [`scope`].
        pub dm_reads: u64,
        pub dm_writes: u64,
    }

    impl FrameTally {
        /// True when nothing was recorded in the frame.
        pub fn is_empty(&self) -> bool {
            self.steps.is_empty() && self.drivers.is_empty() && self.dm_reads == 0 && self.dm_writes == 0
        }
    }

    /// Whether the profile is armed: `EUSTRESS_SCRIPT_PROFILE` is set and not
    /// empty (read once), or a [`capture`] runs on this thread.
    pub fn armed() -> bool {
        env_armed() || CAPTURING.with(Cell::get)
    }

    fn env_armed() -> bool {
        static ARMED: OnceLock<bool> = OnceLock::new();
        *ARMED.get_or_init(|| {
            std::env::var_os("EUSTRESS_SCRIPT_PROFILE").is_some_and(|value| !value.is_empty())
        })
    }

    thread_local! {
        /// The [`ScriptScope`]s open on this thread.
        static SCOPE_DEPTH: Cell<u32> = const { Cell::new(0) };
        /// Whether a [`capture`] runs on this thread. Every call reads it, so
        /// it is a plain flag kept beside the capture itself.
        static CAPTURING: Cell<bool> = const { Cell::new(false) };
        static CAPTURE: RefCell<Option<Captured>> = const { RefCell::new(None) };
    }

    /// Adds `elapsed` to `script`'s `step` in this frame's tally. `runtime`
    /// names the scripting runtime (`"rune"`, `"luau"`) and is a literal in
    /// the caller's code. Names arrive raw and are sanitized here (see
    /// [`sanitize`]); only a name not seen before allocates.
    pub fn record(runtime: &'static str, script: &str, step: &str, elapsed: Duration) {
        record_into(&TALLY, env_armed(), runtime, script, step, elapsed);
    }

    /// Adds `elapsed` to `runtime`'s driver time for this frame: everything
    /// the runtime did in the frame, its steps included, so the part its steps
    /// did not take shows as the runtime's own time.
    pub fn record_driver(runtime: &'static str, elapsed: Duration) {
        record_driver_into(&TALLY, env_armed(), runtime, elapsed);
    }

    /// While the returned guard lives, DataModel reads and writes on this
    /// thread count as script traffic (see [`dm_read`]). Scopes nest.
    /// Unarmed, the guard does nothing.
    pub fn scope() -> ScriptScope {
        scope_in(env_armed())
    }

    /// The guard from [`scope`]. It is not `Send`: the depth it counts is its
    /// own thread's.
    #[must_use = "the scope ends as soon as this guard drops"]
    pub struct ScriptScope {
        counted: bool,
        _not_send: PhantomData<*const ()>,
    }

    impl Drop for ScriptScope {
        fn drop(&mut self) {
            if self.counted {
                SCOPE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            }
        }
    }

    /// Counts one DataModel read, when armed and inside a [`scope`] on this
    /// thread. The DataModel calls it on every read.
    pub fn dm_read() {
        dm_count(&TALLY, env_armed(), false);
    }

    /// Counts one DataModel write, when armed and inside a [`scope`] on this
    /// thread. The DataModel calls it on every write.
    pub fn dm_write() {
        dm_count(&TALLY, env_armed(), true);
    }

    /// Takes this frame's tally and starts the next one. The engine calls it
    /// once a frame. Empty when `EUSTRESS_SCRIPT_PROFILE` is not set.
    pub fn take_frame() -> FrameTally {
        if !env_armed() {
            return FrameTally::default();
        }
        TALLY.take_frame()
    }

    /// Runs `f` with this thread's records going to a capture instead of the
    /// process tally, and returns what `f` recorded. While `f` runs, [`armed`]
    /// is true on this thread, and [`record`], [`record_driver`], [`dm_read`]
    /// and [`dm_write`] from this thread go to the capture. Other threads are
    /// unaffected, so tests running side by side never see each other's
    /// records. Captures do not nest.
    pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Captured) {
        assert!(
            !CAPTURING.with(Cell::get),
            "script_profile::capture does not nest: this thread is already capturing"
        );
        // Ends the capture even when `f` panics, so the thread is not left armed.
        struct End;
        impl Drop for End {
            fn drop(&mut self) {
                CAPTURING.with(|capturing| capturing.set(false));
                CAPTURE.with(|slot| slot.borrow_mut().take());
            }
        }
        CAPTURE.with(|slot| *slot.borrow_mut() = Some(Captured::default()));
        CAPTURING.with(|capturing| capturing.set(true));
        let end = End;
        let out = f();
        let captured = CAPTURE.with(|slot| slot.borrow_mut().take()).unwrap_or_default();
        drop(end);
        (out, captured)
    }

    /// `name` as every output shows it: `;` and control characters (line
    /// breaks included, and the Unicode line and paragraph separators) become
    /// `_`, so a name adds no frame to a folded stack and no line to a file or
    /// the log, and it keeps at most [`MAX_NAME_CHARS`] characters. Borrowed
    /// when nothing changes.
    pub fn sanitize(name: &str) -> Cow<'_, str> {
        let kept = match name.char_indices().nth(MAX_NAME_CHARS) {
            Some((cut, _)) => &name[..cut],
            None => name,
        };
        if !kept.chars().any(replaced) {
            return Cow::Borrowed(kept);
        }
        Cow::Owned(kept.chars().map(|c| if replaced(c) { '_' } else { c }).collect())
    }

    fn replaced(c: char) -> bool {
        c == ';' || c.is_control() || c == '\u{2028}' || c == '\u{2029}'
    }

    /// The open Space's root, without a trailing separator, for [`file_label`].
    static SPACE_ROOT: RwLock<Option<String>> = RwLock::new(None);

    /// Registers the open Space's root folder, so [`file_label`] can name a
    /// script by its path inside the Space. The engine sets it whenever the
    /// open Space changes; `None` forgets it.
    pub fn set_space_root(root: Option<&std::path::Path>) {
        let root = root.and_then(|root| root.to_str()).and_then(root_text);
        *SPACE_ROOT.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = root;
    }

    /// `root` without its trailing separators. `None` for a drive or file
    /// system root, which would put every file on it inside the Space.
    fn root_text(root: &str) -> Option<String> {
        let trimmed = root.trim_end_matches(['/', '\\']);
        (!trimmed.is_empty() && !trimmed.ends_with(':')).then(|| trimmed.to_string())
    }

    /// How a script file is named in the profile: its path inside the open
    /// Space when it is in it (see [`set_space_root`]), else the file's name
    /// alone, so no folder outside the Space (a user name, say) ever shows;
    /// `name` when there is no file. The label is raw, like every name
    /// [`record`] takes.
    pub fn file_label<'a>(name: &'a str, file: Option<&'a str>) -> &'a str {
        let root = SPACE_ROOT.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        label_in(name, file, root.as_deref())
    }

    fn label_in<'a>(name: &'a str, file: Option<&'a str>, root: Option<&str>) -> &'a str {
        let Some(file) = file.filter(|file| !file.is_empty()) else {
            return name;
        };
        if let Some(inside) = root.and_then(|root| inside_root(file, root)) {
            return inside;
        }
        file.rsplit(['/', '\\']).next().filter(|file_name| !file_name.is_empty()).unwrap_or(name)
    }

    /// `file`'s path below `root`, when `file` is inside it: `root` starts
    /// `file`, compared without ASCII case and with either slash (Windows
    /// paths), and a separator follows it. `None` when the rest climbs back
    /// out through `..`.
    fn inside_root<'a>(file: &'a str, root: &str) -> Option<&'a str> {
        let (file_bytes, root_bytes) = (file.as_bytes(), root.as_bytes());
        if file_bytes.len() <= root_bytes.len() || !is_separator(file_bytes[root_bytes.len()]) {
            return None;
        }
        let same = file_bytes
            .iter()
            .zip(root_bytes)
            .all(|(a, b)| a.eq_ignore_ascii_case(b) || (is_separator(*a) && is_separator(*b)));
        if !same {
            return None;
        }
        let rest = file[root_bytes.len()..].trim_start_matches(['/', '\\']);
        (!rest.is_empty() && !rest.split(['/', '\\']).any(|part| part == "..")).then_some(rest)
    }

    fn is_separator(byte: u8) -> bool {
        byte == b'/' || byte == b'\\'
    }

    // ── The tally ──
    //
    // The public functions above are wrappers that pass the process tally and
    // the environment's arming in, so the tests below can run the same paths
    // on a tally of their own, armed or not, without the environment.

    static TALLY: Tally = Tally::new();

    /// What the runtimes recorded since the last [`take_frame`]. The keys are
    /// the raw names and stay from frame to frame, each beside its sanitized
    /// label, so a name seen before costs no allocation; taking a frame zeroes
    /// the values.
    struct Tally {
        runtimes: Mutex<Vec<RuntimeSlot>>,
        dm_reads: AtomicU64,
        dm_writes: AtomicU64,
    }

    struct RuntimeSlot {
        runtime: &'static str,
        driver: Duration,
        scripts: HashMap<Box<str>, ScriptSlot>,
    }

    struct ScriptSlot {
        label: Arc<str>,
        steps: HashMap<Box<str>, StepSlot>,
    }

    struct StepSlot {
        label: Arc<str>,
        total: Duration,
        calls: u64,
    }

    impl Tally {
        const fn new() -> Self {
            Self {
                runtimes: Mutex::new(Vec::new()),
                dm_reads: AtomicU64::new(0),
                dm_writes: AtomicU64::new(0),
            }
        }

        fn runtimes(&self) -> MutexGuard<'_, Vec<RuntimeSlot>> {
            // A panic while the lock was held can cost one step's sums at
            // worst; the profile keeps counting rather than going dark.
            self.runtimes.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn record(&self, runtime: &'static str, script: &str, step: &str, elapsed: Duration) {
            let mut runtimes = self.runtimes();
            let slot = runtime_slot(&mut runtimes, runtime);
            let script_slot = match slot.scripts.get_mut(script) {
                Some(found) => found,
                None => slot.scripts.entry(Box::from(script)).or_insert_with(|| ScriptSlot {
                    label: Arc::from(sanitize(script)),
                    steps: HashMap::new(),
                }),
            };
            let step_slot = match script_slot.steps.get_mut(step) {
                Some(found) => found,
                None => script_slot.steps.entry(Box::from(step)).or_insert_with(|| StepSlot {
                    label: Arc::from(sanitize(step)),
                    total: Duration::ZERO,
                    calls: 0,
                }),
            };
            step_slot.total += elapsed;
            step_slot.calls += 1;
        }

        fn take_frame(&self) -> FrameTally {
            let mut frame = FrameTally::default();
            for slot in self.runtimes().iter_mut() {
                if !slot.driver.is_zero() {
                    frame.drivers.push((slot.runtime, std::mem::take(&mut slot.driver)));
                }
                for script in slot.scripts.values_mut() {
                    for step in script.steps.values_mut().filter(|step| step.calls > 0) {
                        frame.steps.push(StepTotal {
                            runtime: slot.runtime,
                            script: script.label.clone(),
                            step: step.label.clone(),
                            total: std::mem::take(&mut step.total),
                            calls: std::mem::take(&mut step.calls),
                        });
                    }
                }
            }
            frame.dm_reads = self.dm_reads.swap(0, Relaxed);
            frame.dm_writes = self.dm_writes.swap(0, Relaxed);
            frame
        }
    }

    fn runtime_slot<'a>(runtimes: &'a mut Vec<RuntimeSlot>, runtime: &'static str) -> &'a mut RuntimeSlot {
        match runtimes.iter().position(|slot| slot.runtime == runtime) {
            Some(at) => &mut runtimes[at],
            None => {
                runtimes.push(RuntimeSlot { runtime, driver: Duration::ZERO, scripts: HashMap::new() });
                runtimes.last_mut().expect("a runtime slot was just pushed")
            }
        }
    }

    fn with_capture(f: impl FnOnce(&mut Captured)) {
        CAPTURE.with(|slot| {
            if let Some(captured) = slot.borrow_mut().as_mut() {
                f(captured);
            }
        });
    }

    fn record_into(
        tally: &Tally,
        env_armed: bool,
        runtime: &'static str,
        script: &str,
        step: &str,
        elapsed: Duration,
    ) {
        if CAPTURING.with(Cell::get) {
            with_capture(|captured| {
                captured.records.push(Record {
                    runtime,
                    script: sanitize(script).into_owned(),
                    step: sanitize(step).into_owned(),
                    elapsed,
                })
            });
        } else if env_armed {
            tally.record(runtime, script, step, elapsed);
        }
    }

    fn record_driver_into(tally: &Tally, env_armed: bool, runtime: &'static str, elapsed: Duration) {
        if CAPTURING.with(Cell::get) {
            with_capture(|captured| captured.drivers.push((runtime, elapsed)));
        } else if env_armed {
            runtime_slot(&mut tally.runtimes(), runtime).driver += elapsed;
        }
    }

    fn scope_in(env_armed: bool) -> ScriptScope {
        let counted = env_armed || CAPTURING.with(Cell::get);
        if counted {
            SCOPE_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        }
        ScriptScope { counted, _not_send: PhantomData }
    }

    fn dm_count(tally: &Tally, env_armed: bool, write: bool) {
        let capturing = CAPTURING.with(Cell::get);
        if !(env_armed || capturing) || SCOPE_DEPTH.with(Cell::get) == 0 {
            return;
        }
        if capturing {
            with_capture(|captured| {
                if write {
                    captured.dm_writes += 1;
                } else {
                    captured.dm_reads += 1;
                }
            });
        } else if write {
            tally.dm_writes.fetch_add(1, Relaxed);
        } else {
            tally.dm_reads.fetch_add(1, Relaxed);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const MS: Duration = Duration::from_millis(1);

        /// A label a hostile script could choose: markup, quotes, a frame
        /// separator and a line break.
        const HOSTILE: &str = "<script>alert(1)</script>\"&;fake;frame\nfake line";

        #[test]
        fn sanitize_replaces_separators_and_control_characters() {
            assert_eq!(sanitize("a;b\nc\rd\te\u{7f}f\u{0}g\u{85}h\u{2028}i"), "a_b_c_d_e_f_g_h_i");
            assert_eq!(sanitize(HOSTILE), "<script>alert(1)</script>\"&_fake_frame_fake line");
            assert!(matches!(sanitize("Workspace/Car/Drive.rune"), Cow::Borrowed(_)));
            assert!(matches!(sanitize("a;b"), Cow::Owned(_)));
        }

        #[test]
        fn sanitize_keeps_at_most_96_characters_on_a_char_boundary() {
            let long: String = "é".repeat(200);
            let kept = sanitize(&long);
            assert_eq!(kept.chars().count(), MAX_NAME_CHARS);
            assert!(kept.chars().all(|c| c == 'é'));
            let exact: String = "x".repeat(MAX_NAME_CHARS);
            assert_eq!(sanitize(&exact), exact.as_str());
            assert_eq!(sanitize(&format!("{exact};")), exact.as_str());
        }

        #[test]
        fn a_capture_sees_its_threads_records_sanitized() {
            let ((), captured) = capture(|| {
                assert!(armed(), "a capture arms its own thread");
                record("luau", HOSTILE, "Heart;beat\n", 3 * MS);
                record_driver("luau", 5 * MS);
            });
            assert_eq!(
                captured.records,
                vec![Record {
                    runtime: "luau",
                    script: sanitize(HOSTILE).into_owned(),
                    step: "Heart_beat_".to_string(),
                    elapsed: 3 * MS,
                }]
            );
            assert_eq!(captured.drivers, vec![("luau", 5 * MS)]);
            assert!(!CAPTURING.with(Cell::get), "the capture ended with its closure");
        }

        #[test]
        fn a_capture_does_not_see_other_threads() {
            let ((), captured) = capture(|| {
                std::thread::spawn(|| {
                    assert!(!CAPTURING.with(Cell::get), "a capture arms only its own thread");
                    record("rune", "Other", "on_update", MS);
                })
                .join()
                .unwrap();
                record("rune", "Mine", "on_update", MS);
            });
            let scripts: Vec<&str> = captured.records.iter().map(|r| r.script.as_str()).collect();
            assert_eq!(scripts, vec!["Mine"]);
        }

        #[test]
        #[should_panic(expected = "does not nest")]
        fn captures_do_not_nest() {
            let _ = capture(|| capture(|| ()));
        }

        #[test]
        fn a_panic_inside_a_capture_ends_it() {
            let caught = std::panic::catch_unwind(|| capture(|| panic!("script failed")));
            assert!(caught.is_err());
            assert!(!CAPTURING.with(Cell::get), "the thread is not left capturing");
            assert!(CAPTURE.with(|slot| slot.borrow().is_none()));
            let ((), captured) = capture(|| record("rune", "Next", "on_init", MS));
            assert_eq!(captured.records.len(), 1, "a later capture starts clean");
        }

        #[test]
        fn dm_traffic_counts_only_inside_a_scope() {
            let ((), captured) = capture(|| {
                dm_read();
                {
                    let _outer = scope();
                    dm_read();
                    {
                        let _inner = scope();
                        dm_write();
                    }
                    dm_write();
                }
                dm_read();
                dm_write();
            });
            assert_eq!((captured.dm_reads, captured.dm_writes), (1, 2));
            assert_eq!(SCOPE_DEPTH.with(Cell::get), 0, "every scope closed");
        }

        #[test]
        fn unarmed_calls_record_nothing() {
            let tally = Tally::new();
            record_into(&tally, false, "rune", "Car", "on_update", MS);
            record_driver_into(&tally, false, "rune", MS);
            let scope = scope_in(false);
            assert_eq!(SCOPE_DEPTH.with(Cell::get), 0, "an unarmed scope counts nothing");
            dm_count(&tally, false, false);
            dm_count(&tally, false, true);
            drop(scope);
            assert!(tally.runtimes().is_empty(), "no key was made");
            assert!(tally.take_frame().is_empty());
        }

        #[test]
        fn armed_calls_reach_the_tally() {
            let tally = Tally::new();
            record_into(&tally, true, "rune", "Car", "on_update", MS);
            record_driver_into(&tally, true, "rune", 2 * MS);
            {
                let _scope = scope_in(true);
                dm_count(&tally, true, false);
                dm_count(&tally, true, true);
            }
            dm_count(&tally, true, false);
            let frame = tally.take_frame();
            assert_eq!(frame.steps.len(), 1);
            assert_eq!(frame.drivers, vec![("rune", 2 * MS)]);
            assert_eq!((frame.dm_reads, frame.dm_writes), (1, 1));
        }

        #[test]
        fn a_frame_sums_each_step_and_the_next_starts_at_zero() {
            let tally = Tally::new();
            for _ in 0..3 {
                tally.record("rune", "Car", "on_update", 2 * MS);
            }
            tally.record("rune", "Car", "on_init", MS);
            tally.record("luau", "Car", "Heartbeat", 4 * MS);
            let mut steps = tally.take_frame().steps;
            steps.sort_by(|a, b| (a.runtime, &*a.step).cmp(&(b.runtime, &*b.step)));
            let got: Vec<(&str, &str, &str, Duration, u64)> =
                steps.iter().map(|s| (s.runtime, &*s.script, &*s.step, s.total, s.calls)).collect();
            assert_eq!(
                got,
                vec![
                    ("luau", "Car", "Heartbeat", 4 * MS, 1),
                    ("rune", "Car", "on_init", MS, 1),
                    ("rune", "Car", "on_update", 6 * MS, 3),
                ]
            );
            assert!(tally.take_frame().is_empty(), "taking a frame zeroes it");
            tally.record("rune", "Car", "on_update", MS);
            let again = tally.take_frame();
            assert_eq!((again.steps.len(), again.steps[0].calls), (1, 1));
        }

        #[test]
        fn keys_stay_across_frames() {
            let tally = Tally::new();
            let keys = |tally: &Tally| -> usize {
                tally.runtimes().iter().map(|r| r.scripts.values().map(|s| s.steps.len()).sum::<usize>()).sum()
            };
            tally.record("rune", "Car", "on_update", MS);
            tally.record("rune", "Car", "on_init", MS);
            let _ = tally.take_frame();
            assert_eq!(keys(&tally), 2, "taking a frame keeps the keys");
            tally.record("rune", "Car", "on_update", MS);
            assert_eq!(keys(&tally), 2, "a name seen before makes no key");
            tally.record("rune", "Truck", "on_update", MS);
            assert_eq!(keys(&tally), 3);
        }

        #[test]
        fn a_raw_name_is_labelled_sanitized_in_the_tally() {
            let tally = Tally::new();
            tally.record("luau", HOSTILE, "a;b", MS);
            let frame = tally.take_frame();
            assert_eq!(&*frame.steps[0].script, sanitize(HOSTILE).as_ref());
            assert_eq!(&*frame.steps[0].step, "a_b");
        }

        #[test]
        fn drivers_and_dm_counts_reset_when_taken() {
            let tally = Tally::new();
            record_driver_into(&tally, true, "rune", MS);
            record_driver_into(&tally, true, "rune", 2 * MS);
            record_driver_into(&tally, true, "luau", MS);
            tally.dm_reads.fetch_add(4, Relaxed);
            let mut frame = tally.take_frame();
            frame.drivers.sort();
            assert_eq!(frame.drivers, vec![("luau", MS), ("rune", 3 * MS)]);
            assert_eq!(frame.dm_reads, 4);
            assert!(tally.take_frame().is_empty());
        }

        #[test]
        fn a_file_inside_the_space_is_named_by_its_path_there() {
            let root = Some(r"C:\Universe\Spaces\Garage");
            let name = "Driver";
            let label = |file: &'static str| label_in(name, Some(file), root);
            assert_eq!(label(r"C:\Universe\Spaces\Garage\Workspace\Car\Drive.rune"), r"Workspace\Car\Drive.rune");
            assert_eq!(label("c:/universe/spaces/garage/Workspace/Drive.rune"), "Workspace/Drive.rune");
            assert_eq!(label(r"C:\Universe\Spaces\Garage\\Drive.rune"), "Drive.rune");
        }

        #[test]
        fn a_file_outside_the_space_is_named_by_its_file_name_alone() {
            let root = Some(r"C:\Universe\Spaces\Garage");
            let label = |file: &'static str| label_in("Driver", Some(file), root);
            assert_eq!(label(r"C:\Users\someone\Documents\Drive.rune"), "Drive.rune");
            assert_eq!(label(r"C:\Universe\Spaces\GarageTwo\Drive.rune"), "Drive.rune");
            assert_eq!(label(r"C:\Universe\Spaces\Garage\..\..\..\Users\someone\Drive.rune"), "Drive.rune");
            assert_eq!(label_in("Driver", Some("/home/someone/Drive.rune"), None), "Drive.rune");
            assert_eq!(label_in("Driver", None, root), "Driver");
            assert_eq!(label_in("Driver", Some(""), root), "Driver");
            assert_eq!(label_in("Driver", Some(r"C:\Scripts\"), root), "Driver");
        }

        #[test]
        fn a_drive_root_is_no_space_root() {
            assert_eq!(root_text(r"C:\"), None);
            assert_eq!(root_text("/"), None);
            assert_eq!(root_text(r"C:\Universe\Spaces\Garage\"), Some(r"C:\Universe\Spaces\Garage".to_string()));
        }

        #[test]
        fn the_registered_space_root_names_its_files() {
            // A root no other test's files are under, so tests running beside
            // this one label their files the same whether or not it is set.
            let root = std::path::Path::new(r"Q:\script_profile_test\Spaces\Garage");
            set_space_root(Some(root));
            let inside = file_label("Driver", Some(r"Q:\script_profile_test\Spaces\Garage\Workspace\Drive.rune"));
            let outside = file_label("Driver", Some(r"Q:\script_profile_test\Users\someone\Drive.rune"));
            set_space_root(None);
            assert_eq!(inside, r"Workspace\Drive.rune");
            assert_eq!(outside, "Drive.rune");
        }
    }
}

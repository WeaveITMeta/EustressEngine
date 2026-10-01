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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_name() {
        let existing = vec!["Cube".to_string(), "Cube 2".to_string()];
        assert_eq!(unique_name("Cube", &existing), "Cube 3");
        assert_eq!(unique_name("Sphere", &existing), "Sphere");
    }
}

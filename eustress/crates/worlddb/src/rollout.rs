//! Headless batch rollout — "run these N branches forward K ticks, no
//! rendering, return state digests."
//!
//! This is the simulate corner of the AI decision loop: fork N
//! [`BranchHandle`]s off one base world (each carrying a different
//! perturbation), advance every branch K ticks **without any render or
//! windowing machinery**, and hand back a deterministic digest per
//! future so the planner can score / dedupe / pick a winner — then
//! `commit()` exactly one branch and `discard()` the rest.
//!
//! ## Where the tick comes from
//!
//! The storage crate cannot own physics — Avian, Rune scripts, and the
//! ECS schedule live in the engine. So the rollout takes the tick as a
//! caller-supplied `step(branch, tick)` function and owns everything
//! around it: the worker pool, per-branch error isolation, timing, and
//! digesting. The engine wires its headless schedule in as `step`; a
//! test or CLI tool passes a closure. One seam, no render dependency
//! anywhere in the loop.
//!
//! ## Parallelism
//!
//! Branches run concurrently on `min(N, available_parallelism)` worker
//! threads (scoped — no detached threads, results can borrow `step`).
//! Branch overlays are independent so there is no cross-branch
//! contention; reads hit the shared parent, which is `Sync` by the
//! [`WorldDb`](crate::WorldDb) contract. A branch whose `step` errors
//! stops ticking and reports the error; the other branches keep going.

use std::sync::Mutex;

use crate::branch::BranchHandle;
use crate::error::Result;

/// Outcome of one branch's rollout. Returned in input order alongside
/// the (still-live) branch so the caller can commit the winner.
#[derive(Debug, Clone)]
pub struct RolloutOutcome {
    /// Index of this branch in the input `Vec` — outcomes come back in
    /// input order, but the index also survives any future reordering.
    pub index: usize,
    /// Ticks actually executed — equals the requested count unless
    /// `step` errored partway.
    pub ticks_run: u32,
    /// Deterministic digest of the branch's effective overlay after the
    /// rollout ([`BranchHandle::digest`] as lowercase hex). Two futures
    /// that converged to the same perturbation set hash equal.
    pub digest: String,
    /// Overlay entry count after the rollout — the "how much did this
    /// future diverge" size signal.
    pub overlay_len: usize,
    /// Wall-clock the rollout spent inside `step` calls for this branch.
    pub elapsed: std::time::Duration,
    /// `Some(message)` when `step` errored; `ticks_run` then says how
    /// far the branch got. The branch is still returned (its state up
    /// to the failed tick may still be diagnostic).
    pub error: Option<String>,
}

/// Run every branch forward `ticks` ticks via `step`, in parallel,
/// headless. Returns `(branch, outcome)` pairs **in input order**.
///
/// `step(branch, tick)` advances one branch by one tick — the engine
/// passes its headless schedule (physics, scripts, whatever the
/// simulation needs); tick indices run `0..ticks`. The branches are
/// returned alive: score the outcomes, [`BranchHandle::commit`] the
/// winner, [`BranchHandle::discard`] the rest.
///
/// ```no_run
/// # use std::sync::Arc;
/// # use eustress_worlddb::{WorldDb, branch::WorldDbBranchExt, rollout::batch_rollout};
/// # fn demo(world: Arc<dyn WorldDb>) {
/// let branches = (0..12).map(|_| world.branch()).collect();
/// // ... apply one candidate perturbation per branch ...
/// let results = batch_rollout(branches, 60, &|branch, _tick| {
///     // engine-supplied headless tick (physics step, script step, …)
///     # let _ = branch; Ok(())
/// });
/// let winner = results
///     .iter()
///     .position(|(_, o)| o.error.is_none() /* && best score */)
///     .unwrap();
/// for (i, (branch, _)) in results.into_iter().enumerate() {
///     if i == winner { branch.commit().unwrap(); } else { branch.discard(); }
/// }
/// # }
/// ```
pub fn batch_rollout<S>(
    branches: Vec<BranchHandle>,
    ticks: u32,
    step: &S,
) -> Vec<(BranchHandle, RolloutOutcome)>
where
    S: Fn(&BranchHandle, u32) -> Result<()> + Send + Sync,
{
    let n = branches.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(n);

    // Work queue: indexed branches behind a mutex; workers pull until
    // empty. Results land in a slot vec so output preserves input order.
    let queue: Mutex<Vec<(usize, BranchHandle)>> =
        Mutex::new(branches.into_iter().enumerate().rev().collect());
    let results: Mutex<Vec<Option<(BranchHandle, RolloutOutcome)>>> =
        Mutex::new((0..n).map(|_| None).collect());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some((index, branch)) = queue.lock().unwrap().pop() else {
                    break;
                };
                let started = std::time::Instant::now();
                let mut ticks_run = 0u32;
                let mut error = None;
                for tick in 0..ticks {
                    match step(&branch, tick) {
                        Ok(()) => ticks_run += 1,
                        Err(e) => {
                            error = Some(e.to_string());
                            break;
                        }
                    }
                }
                let outcome = RolloutOutcome {
                    index,
                    ticks_run,
                    digest: branch.digest_hex(),
                    overlay_len: branch.overlay_len(),
                    elapsed: started.elapsed(),
                    error,
                };
                results.lock().unwrap()[index] = Some((branch, outcome));
            });
        }
    });

    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|slot| slot.expect("every queued branch produces a result"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Commit, EntityId, WorldDb};
    use crate::branch::WorldDbBranchExt;
    use crate::fjall_backend::FjallWorldDb;
    use crate::keys::ComponentTypeId;
    use std::sync::Arc;

    fn fresh_parent() -> (Arc<dyn WorldDb>, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "eustress_rollout_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db: Arc<dyn WorldDb> = Arc::new(FjallWorldDb::open(&tmp).unwrap());
        (db, tmp)
    }

    #[test]
    fn rollout_runs_all_branches_isolated_and_in_order() {
        let (parent, tmp) = fresh_parent();
        // 8 branches, each perturbing entity i; tick increments a counter
        // component K times.
        let branches: Vec<_> = (0..8u64)
            .map(|i| {
                let b = parent.branch();
                let mut c = Commit::new();
                c.put_component(EntityId(i), ComponentTypeId::TAGS, vec![i as u8]);
                b.apply_commit(c).unwrap();
                b
            })
            .collect();

        let results = batch_rollout(branches, 5, &|branch, tick| {
            // Each tick rewrites the "transform" with the tick number —
            // a stand-in for a physics step mutating state.
            let mut c = Commit::new();
            c.put_component(
                EntityId(0),
                ComponentTypeId::TRANSFORM,
                vec![tick as u8],
            );
            branch.apply_commit(c).map(|_| ())
        });

        assert_eq!(results.len(), 8);
        for (i, (branch, outcome)) in results.iter().enumerate() {
            assert_eq!(outcome.index, i, "outcomes preserve input order");
            assert_eq!(outcome.ticks_run, 5);
            assert!(outcome.error.is_none());
            // Final tick value visible on the branch.
            assert_eq!(
                branch
                    .get_component(EntityId(0), ComponentTypeId::TRANSFORM)
                    .unwrap()
                    .as_deref(),
                Some(&[4u8][..])
            );
        }
        // Digests differ across branches (different seed perturbation)…
        let d0 = &results[0].1.digest;
        let d1 = &results[1].1.digest;
        assert_ne!(d0, d1);
        // …and the parent never saw any of it.
        assert_eq!(
            parent
                .get_component(EntityId(0), ComponentTypeId::TRANSFORM)
                .unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn erroring_branch_reports_without_poisoning_others() {
        let (parent, tmp) = fresh_parent();
        let branches: Vec<_> = (0..3).map(|_| parent.branch()).collect();

        // Branch index is recoverable from a marker each branch carries.
        for (i, b) in branches.iter().enumerate() {
            let mut c = Commit::new();
            c.put_component(EntityId(99), ComponentTypeId::TAGS, vec![i as u8]);
            b.apply_commit(c).unwrap();
        }

        let results = batch_rollout(branches, 10, &|branch, tick| {
            let marker = branch
                .get_component(EntityId(99), ComponentTypeId::TAGS)?
                .unwrap()[0];
            if marker == 1 && tick == 3 {
                return Err(crate::error::Error::Other("blew up".into()));
            }
            Ok(())
        });

        assert!(results[0].1.error.is_none());
        assert_eq!(results[0].1.ticks_run, 10);
        assert_eq!(results[1].1.error.as_deref(), Some("worlddb: blew up"));
        assert_eq!(results[1].1.ticks_run, 3);
        assert!(results[2].1.error.is_none());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn commit_winner_discard_rest() {
        let (parent, tmp) = fresh_parent();
        let branches: Vec<_> = (0..4u64)
            .map(|i| {
                let b = parent.branch();
                let mut c = Commit::new();
                c.put_component(EntityId(i), ComponentTypeId::BASE_PART, vec![i as u8]);
                b.apply_commit(c).unwrap();
                b
            })
            .collect();

        let results = batch_rollout(branches, 1, &|_b, _t| Ok(()));
        // Pick branch 2 as the "winner".
        for (i, (branch, _)) in results.into_iter().enumerate() {
            if i == 2 {
                branch.commit().unwrap();
            } else {
                branch.discard();
            }
        }
        // Only the winner's perturbation reached the parent.
        for i in 0..4u64 {
            let v = parent
                .get_component(EntityId(i), ComponentTypeId::BASE_PART)
                .unwrap();
            if i == 2 {
                assert_eq!(v.as_deref(), Some(&[2u8][..]));
            } else {
                assert_eq!(v, None);
            }
        }
        let _ = std::fs::remove_dir_all(tmp);
    }
}

//! One pool for the RDO passes' chunks across levels (and textures), longest first.
//!
//! Every RDO pass splits a level into chunks of a fixed byte size and optimizes each chunk on its
//! own, block after block. A chunk's output depends only on its own blocks, so it does not matter
//! when or on which thread it runs: the chunk boundaries, a fixed function of each level's size,
//! are all the output depends on. Run level by level, a pass waits at the end of every level for
//! its last chunks while other threads idle (one Kodak image is 6 BC7 chunks at the top level and
//! 2 at the next; 24 BC5 chunks on 20 threads run as two rounds). [`run`] takes every level's
//! chunks at once and hands them out longest first, so small levels fill the gaps beside large
//! ones.

use crate::rdo::RdoStats;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One chunk's work.
pub(crate) struct Task<'a> {
    /// Relative cost, for the longest-first order (its block count).
    pub cost: usize,
    pub run: Box<dyn FnOnce() -> RdoStats + Send + 'a>,
}

impl<'a> Task<'a> {
    pub fn new(cost: usize, run: impl FnOnce() -> RdoStats + Send + 'a) -> Self {
        Self {
            cost,
            run: Box::new(run),
        }
    }
}

/// Runs every job's tasks (a job is one level's chunks) in one pool, longest task first, and returns
/// each job's statistics, summed in task order so they do not depend on the schedule either.
pub(crate) fn run(jobs: Vec<Vec<Task<'_>>>) -> Vec<RdoStats> {
    let count = jobs.len();
    let mut costs = Vec::new();
    let mut owner = Vec::new();
    let mut tasks = Vec::new();
    for (j, job) in jobs.into_iter().enumerate() {
        for t in job {
            costs.push(t.cost);
            owner.push(j);
            tasks.push(Mutex::new(Some(t.run)));
        }
    }
    let n = tasks.len();
    // Stable: equal costs keep their order (earlier levels, then earlier chunks).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(costs[i]));
    let results: Vec<Mutex<RdoStats>> = (0..n).map(|_| Mutex::new(RdoStats::default())).collect();
    let next = AtomicUsize::new(0);
    let work = || {
        loop {
            let k = next.fetch_add(1, Ordering::Relaxed);
            if k >= n {
                break;
            }
            let i = order[k];
            let f = tasks[i].lock().unwrap().take().expect("each task runs once");
            *results[i].lock().unwrap() = f();
        }
    };
    // Rayon falls back to one thread where it has no pool (wasm without threads).
    let workers = rayon::current_num_threads().min(n);
    if workers <= 1 {
        work();
    } else {
        rayon::scope(|s| {
            for _ in 0..workers {
                s.spawn(|_| work());
            }
        });
    }
    let mut out = vec![RdoStats::default(); count];
    for (i, r) in results.into_iter().enumerate() {
        out[owner[i]] = RdoStats::merge(out[owner[i]], r.into_inner().unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every task runs once, on whatever thread, and each job gets its own tasks' sums.
    #[test]
    fn runs_each_task_once_per_job() {
        let runs: Vec<AtomicUsize> = (0..40).map(|_| AtomicUsize::new(0)).collect();
        let jobs: Vec<Vec<Task>> = (0..5)
            .map(|j| {
                (0..8)
                    .map(|k| {
                        let r = &runs[j * 8 + k];
                        Task::new(k * 7 % 5 + j, move || {
                            r.fetch_add(1, Ordering::Relaxed);
                            RdoStats {
                                blocks: j * 100 + k,
                                changed: 1,
                                ..Default::default()
                            }
                        })
                    })
                    .collect()
            })
            .chain(std::iter::once(Vec::new()))
            .collect();
        let stats = run(jobs);
        assert_eq!(stats.len(), 6);
        for (j, s) in stats.iter().enumerate().take(5) {
            assert_eq!((s.blocks, s.changed), (j * 800 + 28, 8));
        }
        assert_eq!(stats[5].blocks, 0);
        assert!(runs.iter().all(|r| r.load(Ordering::Relaxed) == 1));
    }
}

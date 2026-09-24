//! Matches on several threads. Matches are independent (each has its own
//! core, P-space and position seed), so they can run anywhere in any order;
//! the rounds of one match cannot, since P-space carries over from round to
//! round. Threads take the next unplayed match from a shared counter:
//! matches differ widely in length, and a fixed split would leave threads
//! idle while one finishes a long share.
use crate::asm::Config;
use crate::fast::Engine;
use crate::mars::Score;
use crate::multi::Job;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Play every job, `rounds` rounds each, on `threads` threads. Returns the
/// scores in job order and the total number of instructions executed —
/// both the same as playing the jobs one after another.
pub fn play_all(cfg: &Config, jobs: &[Job], rounds: u32, threads: usize) -> (Vec<Score>, u64) {
    let threads = threads.clamp(1, jobs.len().max(1));
    if threads == 1 {
        let mut e = Engine::new(cfg);
        let scores = jobs
            .iter()
            .map(|j| e.play(cfg, j.a, j.b, rounds, j.seed))
            .collect();
        return (scores, e.steps);
    }
    let next = AtomicUsize::new(0);
    let mut scores = vec![Score::default(); jobs.len()];
    let mut steps = 0;
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut e = Engine::new(cfg);
                    let mut done = Vec::new();
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some(j) = jobs.get(k) else { break };
                        done.push((k, e.play(cfg, j.a, j.b, rounds, j.seed)));
                    }
                    (done, e.steps)
                })
            })
            .collect();
        for w in workers {
            let (done, n) = w.join().unwrap();
            for (k, score) in done {
                scores[k] = score;
            }
            steps += n;
        }
    });
    (scores, steps)
}

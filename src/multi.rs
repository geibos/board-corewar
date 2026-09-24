//! Several matches at once, interleaved in one loop.
//!
//! A single battle is a chain of dependent steps: fetch the program counter,
//! then the instruction, then dispatch on it, then the operands. While one
//! chain waits (a load, a mispredicted dispatch), the processor has nothing
//! else to do. Two independent battles interleaved step by step give it a
//! second chain to work on — the same idea SIMD lanes would exploit, done
//! with scalar code, because battles diverge at once and vector lanes would
//! have to execute every opcode for every lane.
//!
//! Rounds of one match depend on each other through P-space (cell 0 holds
//! the previous result), so the lanes run different matches of a round
//! robin. Each lane's own sequence of steps is exactly the sequential one:
//! the result of every match equals `Engine::play` with the same seed
//! (checked by tests/props.rs).

use crate::asm::Config;
use crate::fast::{Compiled, Engine};
use crate::mars::{pmars_rng, Outcome, Score};

/// One match to play: warriors and pMARS-style seed (`-F seed+separation`).
#[derive(Clone, Copy)]
pub struct Job<'a> {
    pub a: &'a Compiled,
    pub b: &'a Compiled,
    pub seed: i32,
}

struct Lane {
    engine: Engine,
    job: Option<usize>,
    round: u32,
    seed: i32,
    score: Score,
    /// Instructions left before the round is a tie.
    budget: u64,
    /// Instructions this round has executed.
    executed: u64,
    /// Warrior 1 moves next (the lane stopped between the two halves of a
    /// pair of moves).
    owes: bool,
}

pub struct Multi {
    lanes: [Lane; 2],
    pub steps: u64,
}

impl Multi {
    pub fn new(cfg: &Config) -> Multi {
        let lane = || Lane {
            engine: Engine::new(cfg),
            job: None,
            round: 0,
            seed: 0,
            score: Score::default(),
            budget: 0,
            executed: 0,
            owes: false,
        };
        Multi {
            lanes: [lane(), lane()],
            steps: 0,
        }
    }

    /// Play every job for `rounds` rounds; scores in job order.
    pub fn play_all(&mut self, cfg: &Config, jobs: &[Job], rounds: u32) -> Vec<Score> {
        let mut results = vec![Score::default(); jobs.len()];
        let mut next = 0usize;
        for l in 0..2 {
            self.start_job(cfg, l, jobs, &mut next, rounds, &mut results);
        }
        loop {
            let active = [self.lanes[0].job.is_some(), self.lanes[1].job.is_some()];
            if !active[0] && !active[1] {
                break;
            }
            if active[0] && active[1] {
                self.both(cfg, jobs, &mut next, rounds, &mut results);
            } else {
                let l = if active[0] { 0 } else { 1 };
                self.single(cfg, l, jobs, &mut next, rounds, &mut results);
            }
        }
        for lane in &mut self.lanes {
            self.steps += std::mem::take(&mut lane.engine.steps);
        }
        results
    }

    fn start_job(
        &mut self,
        cfg: &Config,
        l: usize,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        if *next >= jobs.len() || rounds == 0 {
            self.lanes[l].job = None;
            return;
        }
        let j = *next;
        *next += 1;
        let lane = &mut self.lanes[l];
        lane.engine.begin_match(jobs[j].a, jobs[j].b);
        lane.job = Some(j);
        lane.round = 0;
        lane.seed = jobs[j].seed;
        lane.score = Score::default();
        self.start_round(cfg, l, jobs, next, rounds, results);
    }

    fn start_round(
        &mut self,
        cfg: &Config,
        l: usize,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        let sep = cfg.min_distance;
        let positions = cfg.core_size + 1 - 2 * sep;
        let lane = &mut self.lanes[l];
        let job = jobs[lane.job.unwrap()];
        let pos = sep + (lane.seed.rem_euclid(positions as i32)) as u32;
        lane.seed = pmars_rng(lane.seed);
        lane.engine.start_round([job.a, job.b], [0, pos]);
        lane.budget = cfg.max_cycles as u64 * 2;
        lane.executed = 0;
        // Odd rounds: warrior 1 moves first, which is "owing" a half pair.
        lane.owes = lane.round % 2 == 1;
        self.settle(cfg, l, jobs, next, rounds, results);
    }

    /// Pay a lane's owed move for warrior 1, finishing rounds as needed, so
    /// that the lane is back at "warrior 0 to move".
    fn settle(
        &mut self,
        cfg: &Config,
        l: usize,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        let lane = &mut self.lanes[l];
        if !lane.owes || lane.job.is_none() {
            return;
        }
        lane.owes = false;
        if lane.budget == 0 {
            return self.end_round(cfg, l, Outcome::Tie, jobs, next, rounds, results);
        }
        lane.budget -= 1;
        lane.executed += 1;
        let mut run = lane.engine.view();
        let alive = run.step::<1>();
        let (h, n) = run.queues();
        lane.engine.park(h, n);
        if !alive {
            self.end_round(cfg, l, Outcome::Win(0), jobs, next, rounds, results);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn end_round(
        &mut self,
        cfg: &Config,
        l: usize,
        outcome: Outcome,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        let lane = &mut self.lanes[l];
        lane.engine.finish_round(outcome, lane.executed);
        match outcome {
            Outcome::Win(0) => lane.score.w1 += 1,
            Outcome::Win(_) => lane.score.w2 += 1,
            Outcome::Tie => lane.score.ties += 1,
        }
        lane.round += 1;
        if lane.round == rounds {
            results[lane.job.unwrap()] = lane.score;
            self.start_job(cfg, l, jobs, next, rounds, results);
        } else {
            self.start_round(cfg, l, jobs, next, rounds, results);
        }
    }

    /// Both lanes busy: the interleaved hot loop, until one lane's round ends.
    fn both(
        &mut self,
        cfg: &Config,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        let [l0, l1] = &mut self.lanes;
        let (mut b0, mut b1) = (l0.budget, l1.budget);
        let mut r0 = l0.engine.view();
        let mut r1 = l1.engine.view();
        // (lane that ended, its outcome, whether the other lane owes warrior 1)
        let (ended, outcome, other_owes) = loop {
            if b0 == 0 {
                break (0, Outcome::Tie, false);
            }
            b0 -= 1;
            if !r0.step::<0>() {
                break (0, Outcome::Win(1), false);
            }
            if b1 == 0 {
                break (1, Outcome::Tie, true);
            }
            b1 -= 1;
            if !r1.step::<0>() {
                break (1, Outcome::Win(1), true);
            }
            if b0 == 0 {
                break (0, Outcome::Tie, true);
            }
            b0 -= 1;
            if !r0.step::<1>() {
                break (0, Outcome::Win(0), true);
            }
            if b1 == 0 {
                break (1, Outcome::Tie, false);
            }
            b1 -= 1;
            if !r1.step::<1>() {
                break (1, Outcome::Win(0), false);
            }
        };
        let (h0, n0) = r0.queues();
        let (h1, n1) = r1.queues();
        l0.engine.park(h0, n0);
        l1.engine.park(h1, n1);
        l0.executed += l0.budget - b0;
        l1.executed += l1.budget - b1;
        l0.budget = b0;
        l1.budget = b1;
        let other = 1 - ended;
        self.lanes[other].owes = other_owes;
        self.end_round(cfg, ended, outcome, jobs, next, rounds, results);
        self.settle(cfg, other, jobs, next, rounds, results);
    }

    /// One lane left: finish its current round, then its remaining rounds
    /// and any jobs still queued, without interleaving.
    fn single(
        &mut self,
        cfg: &Config,
        l: usize,
        jobs: &[Job],
        next: &mut usize,
        rounds: u32,
        results: &mut [Score],
    ) {
        let lane = &mut self.lanes[l];
        let mut run = lane.engine.view();
        let (outcome, executed) = run.run(lane.budget, 0);
        let (h, n) = run.queues();
        lane.engine.park(h, n);
        lane.executed += executed;
        lane.budget -= executed;
        self.end_round(cfg, l, outcome, jobs, next, rounds, results);
    }
}

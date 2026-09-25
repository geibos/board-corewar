//! A match on the plain engine with the core's writes recorded: the data
//! behind replays. Rounds are played exactly as `Mars::play` plays them
//! (positions from pMARS's generator, the first mover alternating, P-space
//! carried over); only the rounds asked for are recorded, every round is
//! summarised. Warriors are numbered in argument order: 0 is `a`.
use crate::asm::Config;
use crate::mars::{pmars_rng, Mars, Observer, Outcome, Score};
use crate::redcode::Warrior;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RoundSummary {
    /// From 1.
    pub round: u32,
    /// The warrior that moved first.
    pub first: usize,
    /// Where the second warrior was loaded; the first is at 0.
    pub position: u32,
    /// `None` for a tie.
    pub winner: Option<usize>,
    /// The cycle the round ended on; `cycles` for a tie. A cycle is one
    /// instruction of each warrior.
    pub end_cycle: u32,
}

#[derive(Serialize, Clone, Debug)]
pub struct Frame {
    pub cycle: u32,
    /// Processes of each warrior at this cycle.
    pub processes: [usize; 2],
    /// Cells written since the previous frame, each once, by cell, with the
    /// warrior whose instruction wrote it last.
    pub writes: Vec<(u32, usize)>,
}

#[derive(Serialize, Clone, Debug)]
pub struct End {
    pub cycle: u32,
    pub winner: Option<usize>,
    pub processes: [usize; 2],
}

#[derive(Serialize, Clone, Debug)]
pub struct Recording {
    pub round: u32,
    /// Cycles between frames.
    pub every: u32,
    /// (warrior, load address, length) of each warrior.
    pub start: [(usize, u32, usize); 2],
    pub frames: Vec<Frame>,
    pub end: End,
}

#[derive(Serialize, Clone, Debug)]
pub struct Trace {
    pub score: Score,
    pub rounds: Vec<RoundSummary>,
    pub recorded: Vec<Recording>,
}

struct Recorder {
    /// Instructions per frame: two per cycle.
    every_steps: u64,
    pending: BTreeMap<u32, usize>,
    frames: Vec<Frame>,
}

impl Recorder {
    fn flush(&mut self, mars: &Mars, cycle: u32) {
        self.frames.push(Frame {
            cycle,
            processes: [mars.processes(0), mars.processes(1)],
            writes: std::mem::take(&mut self.pending).into_iter().collect(),
        });
    }
}

impl Observer for Recorder {
    fn after_step(&mut self, mars: &Mars, w: usize, written: &[u32], step: u64) {
        for &cell in written {
            self.pending.insert(cell, w);
        }
        if step.is_multiple_of(self.every_steps) {
            self.flush(mars, (step / 2) as u32);
        }
    }
}

/// A match of `rounds` rounds, like `Mars::play`, recording the rounds whose
/// numbers (from 1) are in `record` with a frame every `every` cycles.
/// `a` is assembled as the first warrior, `b` as the second (see
/// `Config::first_warrior`).
pub fn trace(
    cfg: &Config,
    a: &Warrior,
    b: &Warrior,
    rounds: u32,
    seed: i32,
    record: &[u32],
    every: u32,
) -> Trace {
    assert!(every > 0, "every is at least one cycle");
    let sep = cfg.min_distance;
    let positions = cfg.core_size + 1 - 2 * sep;
    let mut seed = seed;
    let mut mars = Mars::new(cfg, 2);
    let mut out = Trace {
        score: Score::default(),
        rounds: Vec::new(),
        recorded: Vec::new(),
    };
    mars.begin_match(&[a, b]);
    for r in 0..rounds {
        let pos = sep + (seed.rem_euclid(positions as i32)) as u32;
        seed = pmars_rng(seed);
        let first = (r % 2) as usize;
        let number = r + 1;
        let mut rec = record.contains(&number).then(|| Recorder {
            every_steps: every as u64 * 2,
            pending: BTreeMap::new(),
            frames: Vec::new(),
        });
        let outcome = match rec.as_mut() {
            Some(o) => mars.round_observed(cfg, [a, b], [0, pos], first, o),
            None => mars.round(cfg, [a, b], [0, pos], first),
        };
        let winner = match outcome {
            Outcome::Win(x) => Some(x),
            Outcome::Tie => None,
        };
        match winner {
            Some(0) => out.score.w1 += 1,
            Some(_) => out.score.w2 += 1,
            None => out.score.ties += 1,
        }
        let end_cycle = mars.last_round_steps.div_ceil(2) as u32;
        out.rounds.push(RoundSummary {
            round: number,
            first,
            position: pos,
            winner,
            end_cycle,
        });
        if let Some(mut o) = rec {
            if !o.pending.is_empty() || o.frames.last().map(|f| f.cycle) != Some(end_cycle) {
                o.flush(&mars, end_cycle);
            }
            out.recorded.push(Recording {
                round: number,
                every,
                start: [(0, 0, a.code.len()), (1, pos, b.code.len())],
                frames: o.frames,
                end: End {
                    cycle: end_cycle,
                    winner,
                    processes: [mars.processes(0), mars.processes(1)],
                },
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::assemble;

    fn cfg() -> Config {
        Config::default()
    }
    fn dwarf() -> Warrior {
        assemble(include_str!("../testdata/dwarf.red"), &cfg()).unwrap()
    }
    fn imp() -> Warrior {
        assemble(include_str!("../testdata/imp.red"), &cfg()).unwrap()
    }

    #[test]
    fn scores_like_play() {
        let c = cfg();
        for (a, b, seed) in [(dwarf(), imp(), 7), (imp(), dwarf(), 1234)] {
            let t = trace(&c, &a, &b, 30, seed, &[], 50);
            let s = Mars::new(&c, 2).play(&c, &a, &b, 30, seed);
            assert_eq!(t.score, s);
            assert_eq!(t.rounds.len(), 30);
            let wins = |x| t.rounds.iter().filter(|r| r.winner == Some(x)).count() as u32;
            assert_eq!((wins(0), wins(1)), (s.w1, s.w2));
            assert!(t.recorded.is_empty());
            for (i, r) in t.rounds.iter().enumerate() {
                assert_eq!(r.round, i as u32 + 1);
                assert_eq!(r.first, i % 2);
                assert!(r.end_cycle >= 1 && r.end_cycle <= c.max_cycles);
            }
        }
    }

    #[test]
    fn frames_end_with_the_round() {
        let c = cfg();
        let t = trace(&c, &dwarf(), &imp(), 4, 7, &[1, 2], 10);
        assert_eq!(t.recorded.len(), 2);
        for rec in &t.recorded {
            let summary = &t.rounds[rec.round as usize - 1];
            assert_eq!(rec.end.cycle, summary.end_cycle);
            assert_eq!(rec.end.winner, summary.winner);
            assert_eq!(rec.start[1].1, summary.position);
            let cycles: Vec<u32> = rec.frames.iter().map(|f| f.cycle).collect();
            assert!(
                cycles.windows(2).all(|w| w[0] < w[1]),
                "strictly increasing"
            );
            assert!(cycles.iter().all(|&x| x <= rec.end.cycle));
            for f in &rec.frames {
                assert!(f.cycle % 10 == 0 || f.cycle == rec.end.cycle);
                assert!(
                    f.writes.windows(2).all(|w| w[0].0 < w[1].0),
                    "by cell, once"
                );
                assert!(f
                    .writes
                    .iter()
                    .all(|&(cell, w)| cell < c.core_size && w < 2));
            }
        }
    }

    #[test]
    fn a_tie_runs_to_the_cycle_limit() {
        let c = Config {
            max_cycles: 2000,
            ..cfg()
        };
        let t = trace(&c, &imp(), &imp(), 2, 7, &[1], 100);
        assert_eq!(t.score.ties, 2);
        let rec = &t.recorded[0];
        assert_eq!(rec.end.cycle, 2000);
        assert_eq!(rec.end.winner, None);
        let cycles: Vec<u32> = rec.frames.iter().map(|f| f.cycle).collect();
        assert_eq!(cycles, (1..=20).map(|k| k * 100).collect::<Vec<_>>());
        assert_eq!(rec.end.processes, [1, 1]);
    }

    #[test]
    fn one_frame_when_every_is_longer_than_the_round() {
        let c = Config {
            max_cycles: 2000,
            ..cfg()
        };
        let t = trace(&c, &imp(), &imp(), 1, 7, &[1], 1_000_000);
        let rec = &t.recorded[0];
        assert_eq!(rec.frames.len(), 1);
        assert_eq!(rec.frames[0].cycle, 2000);
    }
}

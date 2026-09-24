//! Properties that hold without any reference simulator.

mod common;

use common::{render, warrior};
use corewar::asm::{assemble, Config};
use corewar::mars::{Mars, Outcome};
use proptest::prelude::*;

fn cfg() -> Config {
    Config::default()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// The assembler reads back exactly what the generator wrote.
    #[test]
    fn assembler_reads_what_was_written((code, start) in warrior()) {
        let c = cfg();
        let w = assemble(&render("w", &code, start), &c).unwrap();
        prop_assert_eq!(w.start as usize, start);
        prop_assert_eq!(w.code.len(), code.len());
        for (got, want) in w.code.iter().zip(&code) {
            prop_assert_eq!(got.op, want.op);
            prop_assert_eq!(got.modifier, want.modifier);
            prop_assert_eq!(got.a_mode, want.a_mode);
            prop_assert_eq!(got.b_mode, want.b_mode);
            prop_assert_eq!(got.a as i64, want.a.rem_euclid(c.core_size as i64));
            prop_assert_eq!(got.b as i64, want.b.rem_euclid(c.core_size as i64));
        }
    }

    /// The core is a ring: moving both warriors by the same distance changes
    /// nothing. Catches any place where an address is not folded.
    #[test]
    fn battles_do_not_depend_on_absolute_addresses(
        (a, sa) in warrior(), (b, sb) in warrior(),
        pos in 100u32..7880, shift in 0u32..8000, first in 0usize..2,
    ) {
        let c = cfg();
        let wa = assemble(&render("a", &a, sa), &c).unwrap();
        let wb = assemble(&render("b", &b, sb), &c).unwrap();
        let mut m = Mars::new(&c, 2);
        let here = m.battle(&c, [&wa, &wb], [0, pos], first);
        let there = m.battle(&c, [&wa, &wb], [shift, (shift + pos) % c.core_size], first);
        prop_assert_eq!(here, there);
    }

    /// Fields stay inside the core and no warrior exceeds the process limit,
    /// at every step.
    #[test]
    fn invariants_hold_at_every_step(
        (a, sa) in warrior(), (b, sb) in warrior(),
        pos in 100u32..7880, procs in 1u32..64,
    ) {
        let mut c = cfg();
        c.max_processes = procs;
        let wa = assemble(&render("a", &a, sa), &c).unwrap();
        let wb = assemble(&render("b", &b, sb), &c).unwrap();
        let mut m = Mars::new(&c, 2);
        m.load(&[&wa, &wb], &[0, pos]);
        for step in 0..4000 {
            let w = step % 2;
            let alive = m.step(w);
            prop_assert!(m.processes(w) <= procs as usize);
            if !alive { break; }
        }
        prop_assert!(m.core.iter().all(|i| i.a < c.core_size && i.b < c.core_size));
    }

    /// Same input, same result.
    #[test]
    fn battles_are_deterministic((a, sa) in warrior(), (b, sb) in warrior(), pos in 100u32..7880) {
        let c = cfg();
        let wa = assemble(&render("a", &a, sa), &c).unwrap();
        let wb = assemble(&render("b", &b, sb), &c).unwrap();
        let r1 = Mars::new(&c, 2).battle(&c, [&wa, &wb], [0, pos], 0);
        let r2 = Mars::new(&c, 2).battle(&c, [&wa, &wb], [0, pos], 0);
        prop_assert_eq!(r1, r2);
        let _ = Outcome::Tie;
    }
}

mod fast_vs_reference {
    use super::common::{render, warrior_of};
    use corewar::asm::{assemble, Config};
    use corewar::fast::{Compiled, Engine};
    use corewar::mars::Mars;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 3000, ..ProptestConfig::default() })]

        /// The fast engine is the reference engine, faster: same outcome of
        /// every round, same number of instructions executed, same core at
        /// the end — across a match of several rounds, so P-space carries.
        #[test]
        fn fast_engine_equals_reference(
            (a, sa) in warrior_of(20), (b, sb) in warrior_of(20),
            rounds in 1u32..4, seed in 0i32..100000,
            cycles in prop_oneof![1u32..60, 1u32..4000, Just(80_000u32)],
            procs in prop_oneof![1u32..8, 1u32..300, Just(8000u32)],
            // 8000 runs the specialised path, anything else the generic one
            core in prop_oneof![2 => Just(8000u32), 1 => 60u32..400, 1 => Just(8192u32), 1 => Just(65535u32)],
        ) {
            let cfg = Config {
                max_cycles: cycles,
                max_processes: procs,
                rounds,
                core_size: core,
                min_distance: (core / 4).min(100),
                ..Config::default()
            };
            let wa = assemble(&render("a", &a, sa), &cfg).unwrap();
            let wb = assemble(&render("b", &b, sb), &cfg).unwrap();
            let mut slow = Mars::new(&cfg, 2);
            let mut fast = Engine::new(&cfg);
            let (ca, cb) = (Compiled::new(&wa), Compiled::new(&wb));
            prop_assert_eq!(slow.play(&cfg, &wa, &wb, rounds, seed), fast.play(&cfg, &ca, &cb, rounds, seed));
            prop_assert_eq!(slow.steps, fast.steps);
            prop_assert_eq!(&slow.core, &fast.core());
        }
    }
}

mod multi_vs_sequential {
    use super::common::{render, warrior_of};
    use corewar::asm::{assemble, Config};
    use corewar::fast::{Compiled, Engine};
    use corewar::multi::{Job, Multi};
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 1000, ..ProptestConfig::default() })]

        /// Interleaving matches changes nothing: every match's score and the
        /// total instruction count equal playing them one after another.
        #[test]
        fn interleaved_equals_sequential(
            ws in prop::collection::vec(warrior_of(12), 2..7),
            rounds in 1u32..5,
            cycles in prop_oneof![1u32..40, 1u32..3000, Just(80_000u32)],
            procs in prop_oneof![1u32..8, Just(8000u32)],
        ) {
            let cfg = Config { max_cycles: cycles, max_processes: procs, rounds, ..Config::default() };
            let compiled: Vec<Compiled> = ws
                .iter()
                .enumerate()
                .map(|(k, (c, s))| Compiled::new(&assemble(&render(&format!("w{}", k), c, *s), &cfg).unwrap()))
                .collect();
            let mut jobs = Vec::new();
            for i in 0..compiled.len() {
                for j in i + 1..compiled.len() {
                    jobs.push(Job { a: &compiled[i], b: &compiled[j], seed: (jobs.len() as i32) * 977 + 1 });
                }
            }
            let mut multi = Multi::new(&cfg);
            let got = multi.play_all(&cfg, &jobs, rounds);
            let mut seq = Engine::new(&cfg);
            let want: Vec<_> = jobs.iter().map(|j| seq.play(&cfg, j.a, j.b, rounds, j.seed)).collect();
            prop_assert_eq!(got, want);
            prop_assert_eq!(multi.steps, seq.steps);
        }
    }
}

mod pool_vs_sequential {
    use super::common::{render, warrior_of};
    use corewar::asm::{assemble, Config};
    use corewar::fast::{Compiled, Engine};
    use corewar::multi::Job;
    use corewar::pool;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

        /// Playing matches on several threads changes nothing: scores come
        /// back in job order and equal playing them one after another, and
        /// the instruction count is the same.
        #[test]
        fn threaded_equals_sequential(
            ws in prop::collection::vec(warrior_of(12), 2..8),
            rounds in 1u32..5,
            cycles in prop_oneof![1u32..40, 1u32..3000, Just(80_000u32)],
            procs in prop_oneof![1u32..8, Just(8000u32)],
            threads in 1usize..9,
        ) {
            let cfg = Config { max_cycles: cycles, max_processes: procs, rounds, ..Config::default() };
            let compiled: Vec<Compiled> = ws
                .iter()
                .enumerate()
                .map(|(k, (c, s))| Compiled::new(&assemble(&render(&format!("w{}", k), c, *s), &cfg).unwrap()))
                .collect();
            let mut jobs = Vec::new();
            for i in 0..compiled.len() {
                for j in i + 1..compiled.len() {
                    jobs.push(Job { a: &compiled[i], b: &compiled[j], seed: (jobs.len() as i32) * 977 + 1 });
                }
            }
            let (got, steps) = pool::play_all(&cfg, &jobs, rounds, threads);
            let mut seq = Engine::new(&cfg);
            let want: Vec<_> = jobs.iter().map(|j| seq.play(&cfg, j.a, j.b, rounds, j.seed)).collect();
            prop_assert_eq!(got, want);
            prop_assert_eq!(steps, seq.steps);
        }
    }
}

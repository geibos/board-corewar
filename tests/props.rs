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

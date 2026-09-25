//! The observer of the reference engine: it sees every write and changes
//! nothing about the round it watches.

mod common;

use common::{render, warrior_of};
use corewar::asm::{assemble, Config};
use corewar::mars::{Mars, Observer};
use corewar::redcode::Instruction;
use proptest::prelude::*;
use std::collections::HashSet;

/// A small core keeps the plain engine quick on random warriors.
fn cfg() -> Config {
    Config {
        core_size: 800,
        max_cycles: 8000,
        max_processes: 64,
        max_length: 20,
        min_distance: 20,
        ..Config::default()
    }
}

#[derive(Default)]
struct Collect {
    before: Vec<Instruction>,
    written: HashSet<u32>,
    last_step: u64,
    movers: Vec<usize>,
}

impl Observer for Collect {
    fn loaded(&mut self, mars: &Mars) {
        self.before = mars.core.clone();
        self.written.clear();
        self.movers.clear();
    }
    fn after_step(&mut self, _mars: &Mars, w: usize, written: &[u32], step: u64) {
        assert_eq!(step, self.last_step + 1, "steps are numbered one by one");
        self.last_step = step;
        self.movers.push(w);
        self.written.extend(written.iter().copied());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// Three rounds of a match, observed and not: the same outcomes, the
    /// same number of instructions, the same core at the end.
    #[test]
    fn observing_changes_nothing(
        (ca, sa) in warrior_of(20),
        (cb, sb) in warrior_of(20),
        pos in 20u32..=780,
    ) {
        let c = cfg();
        let a = assemble(&render("a", &ca, sa), &c).unwrap();
        let b = assemble(&render("b", &cb, sb), &c).unwrap();
        let mut plain = Mars::new(&c, 2);
        let mut seen = Mars::new(&c, 2);
        plain.begin_match(&[&a, &b]);
        seen.begin_match(&[&a, &b]);
        for r in 0..3usize {
            let o1 = plain.round(&c, [&a, &b], [0, pos], r % 2);
            let mut obs = Collect::default();
            let o2 = seen.round_observed(&c, [&a, &b], [0, pos], r % 2, &mut obs);
            prop_assert_eq!(o1, o2);
            prop_assert_eq!(plain.last_round_steps, seen.last_round_steps);
            prop_assert_eq!(obs.last_step, seen.last_round_steps);
            prop_assert!(plain.core == seen.core);
        }
    }

    /// Every cell whose content differs at the end of a round from right
    /// after loading was reported as written.
    #[test]
    fn every_change_is_reported(
        (ca, sa) in warrior_of(20),
        (cb, sb) in warrior_of(20),
        pos in 20u32..=780,
        first in 0usize..2,
    ) {
        let c = cfg();
        let a = assemble(&render("a", &ca, sa), &c).unwrap();
        let b = assemble(&render("b", &cb, sb), &c).unwrap();
        let mut m = Mars::new(&c, 2);
        m.begin_match(&[&a, &b]);
        let mut obs = Collect::default();
        m.round_observed(&c, [&a, &b], [0, pos], first, &mut obs);
        for i in 0..c.core_size {
            if m.core[i as usize] != obs.before[i as usize] {
                prop_assert!(obs.written.contains(&i), "cell {} changed unreported", i);
            }
        }
        prop_assert_eq!(obs.movers.first().copied(), Some(first));
    }
}

/// DIV and MOD by zero kill the process and write nothing: no cell is
/// reported, or a replay would show a write that never happened.
#[test]
fn a_division_by_zero_writes_nothing() {
    let c = cfg();
    let div = assemble(";redcode-94\n;name div\n;author t\n DIV.AB #0, 3\n", &c).unwrap();
    let imp = assemble(include_str!("../testdata/imp.red"), &c).unwrap();
    let mut m = Mars::new(&c, 2);
    m.begin_match(&[&div, &imp]);
    struct FirstStep(Option<Vec<u32>>);
    impl Observer for FirstStep {
        fn after_step(&mut self, _mars: &Mars, _w: usize, written: &[u32], _step: u64) {
            if self.0.is_none() {
                self.0 = Some(written.to_vec());
            }
        }
    }
    let mut obs = FirstStep(None);
    m.round_observed(&c, [&div, &imp], [0, 400], 0, &mut obs);
    assert_eq!(obs.0, Some(vec![]), "DIV.AB #0 wrote nothing");
}

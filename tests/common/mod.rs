//! Proptest strategies shared by the tests: random warriors that use every
//! opcode, modifier and addressing mode, written out as Redcode text.
#![allow(dead_code)]

use corewar::redcode::{Mode, Modifier, Opcode};
use proptest::prelude::*;

#[derive(Clone, Debug)]
pub struct Spec {
    pub op: Opcode,
    pub modifier: Modifier,
    pub a_mode: Mode,
    pub a: i64,
    pub b_mode: Mode,
    pub b: i64,
}

/// Small offsets most of the time, so that warriors touch themselves and
/// each other; sometimes anything in the core.
pub fn field() -> impl Strategy<Value = i64> {
    prop_oneof![
        6 => -10i64..=10,
        2 => -100i64..=100,
        1 => Just(0i64),
        1 => 0i64..8000,
    ]
}

pub fn opcode() -> impl Strategy<Value = Opcode> {
    // Live code weighs more than DAT, so that battles last and interact.
    prop_oneof![
        1 => Just(Opcode::Dat),
        3 => Just(Opcode::Mov),
        3 => Just(Opcode::Spl),
        12 => prop::sample::select(Opcode::ALL.to_vec()),
    ]
}

pub fn spec() -> impl Strategy<Value = Spec> {
    (
        opcode(),
        prop::sample::select(Modifier::ALL.to_vec()),
        prop::sample::select(Mode::ALL.to_vec()),
        field(),
        prop::sample::select(Mode::ALL.to_vec()),
        field(),
    )
        .prop_map(|(op, modifier, a_mode, a, b_mode, b)| Spec {
            op,
            modifier,
            a_mode,
            a,
            b_mode,
            b,
        })
}

/// (instructions, start offset)
pub fn warrior() -> impl Strategy<Value = (Vec<Spec>, usize)> {
    warrior_of(10)
}

/// Warriors of up to `max_len` instructions.
pub fn warrior_of(max_len: usize) -> impl Strategy<Value = (Vec<Spec>, usize)> {
    prop::collection::vec(spec(), 1..=max_len).prop_flat_map(|code| {
        let n = code.len();
        (Just(code), 0..n)
    })
}

pub fn render(name: &str, code: &[Spec], start: usize) -> String {
    let mut s = format!(";redcode-94\n;name {}\n;assert 1\n", name);
    for (i, x) in code.iter().enumerate() {
        s.push_str(&format!(
            "{} {}.{} {}{}, {}{}\n",
            if i == 0 { "top" } else { "   " },
            x.op.name(),
            x.modifier.name(),
            x.a_mode.symbol(),
            x.a,
            x.b_mode.symbol(),
            x.b
        ));
    }
    s.push_str(&format!("    END top+{}\n", start));
    s
}

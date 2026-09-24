//! The fast engine: the same semantics as [`crate::mars::Mars`] (pMARS's),
//! laid out for speed. `Mars` stays as the plain reference and is the oracle
//! this engine is checked against in-process, at millions of cases, besides
//! pMARS itself.
//!
//! Layout:
//! * a cell is 8 bytes: two u16 fields, one byte `opcode * 8 + modifier` so
//!   that dispatch is a single jump table, one byte per mode;
//! * the empty cell `DAT.F $0, $0` is all zeroes, so clearing the core
//!   between rounds is a memset;
//! * process queues are fixed ring buffers, nothing is allocated while
//!   running;
//! * addresses are always folded into the core before use, so core accesses
//!   skip bounds checks.
//!
//! Core sizes up to 65535 (u16 fields); larger cores use `Mars`.

use crate::asm::Config;
use crate::mars::{pmars_rng, Outcome, Score};
use crate::redcode::{Instruction, Mode, Modifier, Opcode, Warrior};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Cell {
    pub a: u16,
    pub b: u16,
    /// opcode * 8 + modifier, see OP_* and M_*
    pub opm: u8,
    pub am: u8,
    pub bm: u8,
    pad: u8,
}

// Encodings chosen so that DAT.F $0, $0 is all zeroes.
const O_DAT: u8 = 0;
const O_MOV: u8 = 1;
const O_ADD: u8 = 2;
const O_SUB: u8 = 3;
const O_MUL: u8 = 4;
const O_DIV: u8 = 5;
const O_MOD: u8 = 6;
const O_JMP: u8 = 7;
const O_JMZ: u8 = 8;
const O_JMN: u8 = 9;
const O_DJN: u8 = 10;
const O_SPL: u8 = 11;
const O_SLT: u8 = 12;
const O_CMP: u8 = 13;
const O_SEQ: u8 = 14;
const O_SNE: u8 = 15;
const O_NOP: u8 = 16;
const O_LDP: u8 = 17;
const O_STP: u8 = 18;

const M_F: u8 = 0;
const M_A: u8 = 1;
const M_B: u8 = 2;
const M_AB: u8 = 3;
const M_BA: u8 = 4;
const M_X: u8 = 5;
const M_I: u8 = 6;

const DIRECT: u8 = 0;
const IMMEDIATE: u8 = 1;
const A_IND: u8 = 2;
const B_IND: u8 = 3;
const A_PRE: u8 = 4;
const B_PRE: u8 = 5;
const A_POST: u8 = 6;
const B_POST: u8 = 7;

fn enc_op(o: Opcode) -> u8 {
    match o {
        Opcode::Dat => O_DAT,
        Opcode::Mov => O_MOV,
        Opcode::Add => O_ADD,
        Opcode::Sub => O_SUB,
        Opcode::Mul => O_MUL,
        Opcode::Div => O_DIV,
        Opcode::Mod => O_MOD,
        Opcode::Jmp => O_JMP,
        Opcode::Jmz => O_JMZ,
        Opcode::Jmn => O_JMN,
        Opcode::Djn => O_DJN,
        Opcode::Spl => O_SPL,
        Opcode::Slt => O_SLT,
        Opcode::Cmp => O_CMP,
        Opcode::Seq => O_SEQ,
        Opcode::Sne => O_SNE,
        Opcode::Nop => O_NOP,
        Opcode::Ldp => O_LDP,
        Opcode::Stp => O_STP,
    }
}
fn enc_mod(m: Modifier) -> u8 {
    match m {
        Modifier::F => M_F,
        Modifier::A => M_A,
        Modifier::B => M_B,
        Modifier::AB => M_AB,
        Modifier::BA => M_BA,
        Modifier::X => M_X,
        Modifier::I => M_I,
    }
}
fn enc_mode(m: Mode) -> u8 {
    match m {
        Mode::Direct => DIRECT,
        Mode::Immediate => IMMEDIATE,
        Mode::AIndirect => A_IND,
        Mode::BIndirect => B_IND,
        Mode::APredec => A_PRE,
        Mode::BPredec => B_PRE,
        Mode::APostinc => A_POST,
        Mode::BPostinc => B_POST,
    }
}

impl Cell {
    pub fn encode(i: &Instruction) -> Cell {
        Cell {
            a: i.a as u16,
            b: i.b as u16,
            opm: enc_op(i.op) * 8 + enc_mod(i.modifier),
            am: enc_mode(i.a_mode),
            bm: enc_mode(i.b_mode),
            pad: 0,
        }
    }

    pub fn decode(&self) -> Instruction {
        let op = [
            Opcode::Dat,
            Opcode::Mov,
            Opcode::Add,
            Opcode::Sub,
            Opcode::Mul,
            Opcode::Div,
            Opcode::Mod,
            Opcode::Jmp,
            Opcode::Jmz,
            Opcode::Jmn,
            Opcode::Djn,
            Opcode::Spl,
            Opcode::Slt,
            Opcode::Cmp,
            Opcode::Seq,
            Opcode::Sne,
            Opcode::Nop,
            Opcode::Ldp,
            Opcode::Stp,
        ][(self.opm / 8) as usize];
        let modifier = [
            Modifier::F,
            Modifier::A,
            Modifier::B,
            Modifier::AB,
            Modifier::BA,
            Modifier::X,
            Modifier::I,
        ][(self.opm % 8) as usize];
        let mode = |m: u8| {
            [
                Mode::Direct,
                Mode::Immediate,
                Mode::AIndirect,
                Mode::BIndirect,
                Mode::APredec,
                Mode::BPredec,
                Mode::APostinc,
                Mode::BPostinc,
            ][m as usize]
        };
        Instruction {
            op,
            modifier,
            a_mode: mode(self.am),
            a: self.a as u32,
            b_mode: mode(self.bm),
            b: self.b as u32,
        }
    }
}

/// A warrior prepared for the fast engine.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub code: Vec<Cell>,
    pub start: u16,
    pub pin: Option<i64>,
}

impl Compiled {
    pub fn new(w: &Warrior) -> Compiled {
        Compiled {
            code: w.code.iter().map(Cell::encode).collect(),
            start: w.start as u16,
            pin: w.pin,
        }
    }
}

/// Everything the hot loop touches, borrowed for one round. Separate `&mut`
/// slices tell the compiler the core, the two queues and P-space never
/// alias, so queue heads and lengths, the core base and its size can live
/// in registers instead of being reloaded from the engine after every store
/// into the core (which is what the profile showed before).
pub(crate) struct Run<'a> {
    core: &'a mut [Cell],
    qbuf: [&'a mut [u16]; 2],
    head: [usize; 2],
    len: [usize; 2],
    mask: usize,
    cs: u32,
    max_processes: usize,
    pspace: &'a mut [Vec<u16>],
    bank: [usize; 2],
    last_result: &'a mut [u16; 2],
    pspace_size: u32,
}

impl Run<'_> {
    /// Queue heads and lengths, to park between segments of a round.
    #[inline(always)]
    pub(crate) fn queues(&self) -> ([usize; 2], [usize; 2]) {
        (self.head, self.len)
    }

    #[inline(always)]
    fn pop<const W: usize>(&mut self) -> u16 {
        let h = self.head[W];
        let v = unsafe { *self.qbuf[W].get_unchecked(h) };
        self.head[W] = (h + 1) & self.mask;
        self.len[W] -= 1;
        v
    }
    #[inline(always)]
    fn push<const W: usize>(&mut self, v: u16) {
        let at = (self.head[W] + self.len[W]) & self.mask;
        unsafe { *self.qbuf[W].get_unchecked_mut(at) = v };
        self.len[W] += 1;
    }

    #[inline(always)]
    fn fold(&self, x: u32) -> u32 {
        if x >= self.cs {
            x - self.cs
        } else {
            x
        }
    }

    #[inline(always)]
    fn cell(&self, i: u32) -> Cell {
        debug_assert!(i < self.cs);
        unsafe { *self.core.get_unchecked(i as usize) }
    }
    #[inline(always)]
    fn cell_mut(&mut self, i: u32) -> &mut Cell {
        debug_assert!(i < self.cs);
        unsafe { self.core.get_unchecked_mut(i as usize) }
    }

    /// (pointer offset, address of the cell pointed to, its (a, b) at
    /// evaluation time). Immediate: the instruction as fetched, at pc.
    #[inline(always)]
    fn operand(&mut self, pc: u32, ir: Cell, mode: u8, field: u16) -> (u32, u32, u16, u16) {
        let cs = self.cs;
        match mode {
            IMMEDIATE => (0, pc, ir.a, ir.b),
            DIRECT => {
                let at = self.fold(pc + field as u32);
                let c = self.cell(at);
                (field as u32, at, c.a, c.b)
            }
            _ => {
                let p = self.fold(pc + field as u32);
                let ind = match mode {
                    A_PRE => {
                        let c = self.cell_mut(p);
                        c.a = if c.a == 0 { (cs - 1) as u16 } else { c.a - 1 };
                        c.a
                    }
                    B_PRE => {
                        let c = self.cell_mut(p);
                        c.b = if c.b == 0 { (cs - 1) as u16 } else { c.b - 1 };
                        c.b
                    }
                    A_IND | A_POST => self.cell(p).a,
                    _ => self.cell(p).b,
                };
                let ptr = self.fold(field as u32 + ind as u32);
                let at = self.fold(pc + ptr);
                let c = self.cell(at);
                match mode {
                    A_POST => {
                        let q = self.cell_mut(p);
                        q.a = if q.a as u32 + 1 == cs { 0 } else { q.a + 1 };
                    }
                    B_POST => {
                        let q = self.cell_mut(p);
                        q.b = if q.b as u32 + 1 == cs { 0 } else { q.b + 1 };
                    }
                    _ => {}
                }
                (ptr, at, c.a, c.b)
            }
        }
    }

    #[inline(always)]
    fn get_pspace<const W: usize>(&self, idx: u16) -> u16 {
        let i = (idx as u32 % self.pspace_size) as usize;
        if i == 0 {
            self.last_result[W]
        } else {
            self.pspace[self.bank[W]][i]
        }
    }

    #[inline(always)]
    fn set_pspace<const W: usize>(&mut self, idx: u16, v: u16) {
        let i = (idx as u32 % self.pspace_size) as usize;
        if i == 0 {
            self.last_result[W] = v;
        } else {
            let b = self.bank[W];
            self.pspace[b][i] = v;
        }
    }

    /// One instruction of warrior `w`; false if it has no processes after.
    #[inline(always)]
    pub(crate) fn step<const W: usize>(&mut self) -> bool {
        let cs = self.cs;
        let pc = self.pop::<W>() as u32;
        let ir = self.cell(pc);
        let (rpa, a_at, aa, ab) = self.operand(pc, ir, ir.am, ir.a);
        let (wpb, _, mut ba, mut bb) = self.operand(pc, ir, ir.bm, ir.b);
        let t = self.fold(pc + wpb);
        // Computed where used: most instructions need only one of them.
        macro_rules! next {
            () => {
                self.fold(pc + 1) as u16
            };
        }
        macro_rules! jump {
            () => {
                self.fold(pc + rpa) as u16
            };
        }
        macro_rules! skip {
            () => {
                self.fold(pc + 2) as u16
            };
        }
        let add = |x: u16, y: u16| -> u16 {
            let s = x as u32 + y as u32;
            (if s >= cs { s - cs } else { s }) as u16
        };
        let sub = |x: u16, y: u16| -> u16 {
            let s = x as u32 + cs - y as u32;
            (if s >= cs { s - cs } else { s }) as u16
        };
        let mul = |x: u16, y: u16| -> u16 { ((x as u32 * y as u32) % cs) as u16 };
        let dec = |x: u16| -> u16 {
            if x == 0 {
                (cs - 1) as u16
            } else {
                x - 1
            }
        };
        let op = ir.opm >> 3;
        let m = ir.opm & 7;
        macro_rules! push {
            ($v:expr) => {
                self.push::<W>($v)
            };
        }
        match op {
            O_DAT => {}
            O_NOP => push!(next!()),
            O_MOV => {
                if m == M_I {
                    let src = self.cell(a_at);
                    *self.cell_mut(t) = Cell {
                        a: aa,
                        b: ab,
                        ..src
                    };
                } else {
                    let c = self.cell_mut(t);
                    match m {
                        M_A => c.a = aa,
                        M_B => c.b = ab,
                        M_AB => c.b = aa,
                        M_BA => c.a = ab,
                        M_F => {
                            c.a = aa;
                            c.b = ab;
                        }
                        _ => {
                            c.a = ab;
                            c.b = aa;
                        }
                    }
                }
                push!(next!());
            }
            O_ADD | O_SUB | O_MUL => {
                let f = |x: u16, y: u16| match op {
                    O_ADD => add(x, y),
                    O_SUB => sub(x, y),
                    _ => mul(x, y),
                };
                let c = self.cell_mut(t);
                match m {
                    M_A => c.a = f(ba, aa),
                    M_B => c.b = f(bb, ab),
                    M_AB => c.b = f(bb, aa),
                    M_BA => c.a = f(ba, ab),
                    M_X => {
                        c.a = f(ba, ab);
                        c.b = f(bb, aa);
                    }
                    _ => {
                        c.a = f(ba, aa);
                        c.b = f(bb, ab);
                    }
                }
                push!(next!());
            }
            O_DIV | O_MOD => {
                let div = op == O_DIV;
                let f = |x: u16, y: u16| -> Option<u16> {
                    if y == 0 {
                        None
                    } else if div {
                        Some(x / y)
                    } else {
                        Some(x % y)
                    }
                };
                let c = self.cell_mut(t);
                let alive = match m {
                    M_A => f(ba, aa).map(|v| c.a = v).is_some(),
                    M_B => f(bb, ab).map(|v| c.b = v).is_some(),
                    M_AB => f(bb, aa).map(|v| c.b = v).is_some(),
                    M_BA => f(ba, ab).map(|v| c.a = v).is_some(),
                    M_X => {
                        let x = f(ba, ab).map(|v| c.a = v).is_some();
                        let y = f(bb, aa).map(|v| c.b = v).is_some();
                        x && y
                    }
                    _ => {
                        let x = f(ba, aa).map(|v| c.a = v).is_some();
                        let y = f(bb, ab).map(|v| c.b = v).is_some();
                        x && y
                    }
                };
                if alive {
                    push!(next!());
                }
            }
            O_JMP => push!(jump!()),
            O_JMZ => {
                let z = match m {
                    M_A | M_BA => ba == 0,
                    M_B | M_AB => bb == 0,
                    _ => ba == 0 && bb == 0,
                };
                push!(if z { jump!() } else { next!() });
            }
            O_JMN => {
                let nz = match m {
                    M_A | M_BA => ba != 0,
                    M_B | M_AB => bb != 0,
                    _ => ba != 0 || bb != 0,
                };
                push!(if nz { jump!() } else { next!() });
            }
            O_DJN => {
                let nz = match m {
                    M_A | M_BA => {
                        let c = self.cell_mut(t);
                        c.a = dec(c.a);
                        ba = dec(ba);
                        ba != 0
                    }
                    M_B | M_AB => {
                        let c = self.cell_mut(t);
                        c.b = dec(c.b);
                        bb = dec(bb);
                        bb != 0
                    }
                    _ => {
                        let c = self.cell_mut(t);
                        c.a = dec(c.a);
                        c.b = dec(c.b);
                        ba = dec(ba);
                        bb = dec(bb);
                        ba != 0 || bb != 0
                    }
                };
                push!(if nz { jump!() } else { next!() });
            }
            O_SPL => {
                push!(next!());
                if self.len[W] < self.max_processes {
                    push!(jump!());
                }
            }
            O_SLT => {
                let lt = match m {
                    M_A => aa < ba,
                    M_B => ab < bb,
                    M_AB => aa < bb,
                    M_BA => ab < ba,
                    M_X => aa < bb && ab < ba,
                    _ => aa < ba && ab < bb,
                };
                push!(if lt { skip!() } else { next!() });
            }
            O_CMP | O_SEQ | O_SNE => {
                let eq = match m {
                    M_A => aa == ba,
                    M_B => ab == bb,
                    M_AB => aa == bb,
                    M_BA => ab == ba,
                    M_F => aa == ba && ab == bb,
                    M_X => aa == bb && ab == ba,
                    _ => {
                        let (x, y) = (self.cell(a_at), self.cell(t));
                        x.opm == y.opm && x.am == y.am && x.bm == y.bm && aa == ba && ab == bb
                    }
                };
                let take = if op == O_SNE { !eq } else { eq };
                push!(if take { skip!() } else { next!() });
            }
            O_LDP => {
                let v = match m {
                    M_A | M_AB => self.get_pspace::<W>(aa),
                    _ => self.get_pspace::<W>(ab),
                };
                match m {
                    M_A | M_BA => self.cell_mut(t).a = v,
                    _ => self.cell_mut(t).b = v,
                }
                push!(next!());
            }
            _ => {
                // O_STP
                match m {
                    M_A => self.set_pspace::<W>(ba, aa),
                    M_AB => self.set_pspace::<W>(bb, aa),
                    M_BA => self.set_pspace::<W>(ba, ab),
                    _ => self.set_pspace::<W>(bb, ab),
                }
                push!(next!());
            }
        }
        self.len[W] != 0
    }

    /// Play until one warrior has no processes or `budget` instructions have
    /// run; unrolled by pairs so each step knows its warrior at compile time.
    /// Returns the outcome and the number of instructions executed.
    #[inline(always)]
    pub(crate) fn run(&mut self, budget: u64, first: usize) -> (Outcome, u64) {
        let mut steps = budget;
        if first == 1 {
            if steps == 0 {
                return (Outcome::Tie, 0);
            }
            steps -= 1;
            if !self.step::<1>() {
                return (Outcome::Win(0), budget - steps);
            }
        }
        loop {
            if steps == 0 {
                return (Outcome::Tie, budget);
            }
            steps -= 1;
            if !self.step::<0>() {
                return (Outcome::Win(1), budget - steps);
            }
            if steps == 0 {
                return (Outcome::Tie, budget);
            }
            steps -= 1;
            if !self.step::<1>() {
                return (Outcome::Win(0), budget - steps);
            }
        }
    }
}

pub struct Engine {
    cs: u32,
    max_processes: usize,
    core: Vec<Cell>,
    qbuf: [Box<[u16]>; 2],
    mask: usize,
    start_pc: [u16; 2],
    /// Queue state between segments of a round (see `crate::multi`).
    head: [usize; 2],
    len: [usize; 2],
    pspace: Vec<Vec<u16>>,
    bank: [usize; 2],
    last_result: [u16; 2],
    pspace_size: u32,
    pub steps: u64,
}

impl Engine {
    pub fn new(cfg: &Config) -> Engine {
        assert!(
            cfg.core_size <= 65535,
            "the fast engine handles cores up to 65535"
        );
        let ps = cfg.pspace_size();
        let cap = (cfg.max_processes as usize + 1).next_power_of_two();
        Engine {
            cs: cfg.core_size,
            max_processes: cfg.max_processes as usize,
            core: vec![Cell::default(); cfg.core_size as usize],
            qbuf: [
                vec![0; cap].into_boxed_slice(),
                vec![0; cap].into_boxed_slice(),
            ],
            mask: cap - 1,
            start_pc: [0; 2],
            head: [0; 2],
            len: [0; 2],
            pspace: vec![vec![0; ps as usize]; 2],
            bank: [0, 1],
            last_result: [(cfg.core_size - 1) as u16; 2],
            pspace_size: ps,
            steps: 0,
        }
    }

    pub fn core(&self) -> Vec<Instruction> {
        self.core.iter().map(Cell::decode).collect()
    }

    pub fn begin_match(&mut self, a: &Compiled, b: &Compiled) {
        let shared = a.pin.is_some() && a.pin == b.pin;
        self.bank = if shared { [0, 0] } else { [0, 1] };
        for bank in &mut self.pspace {
            bank.fill(0);
        }
        self.last_result = [(self.cs - 1) as u16; 2];
    }

    fn load(&mut self, w: [&Compiled; 2], pos: [u32; 2]) {
        self.core.fill(Cell::default());
        for (k, (war, &p)) in w.iter().zip(pos.iter()).enumerate() {
            let mut at = p as usize;
            for c in &war.code {
                self.core[at] = *c;
                at += 1;
                if at == self.cs as usize {
                    at = 0;
                }
            }
            self.start_pc[k] = ((p + war.start as u32) % self.cs) as u16;
        }
    }

    /// Load a round: core cleared, warriors placed, one process each.
    pub(crate) fn start_round(&mut self, w: [&Compiled; 2], pos: [u32; 2]) {
        self.load(w, pos);
        self.qbuf[0][0] = self.start_pc[0];
        self.qbuf[1][0] = self.start_pc[1];
        self.head = [0, 0];
        self.len = [1, 1];
    }

    /// The hot-loop view of this engine's current round.
    #[inline(always)]
    pub(crate) fn view(&mut self) -> Run<'_> {
        let [q0, q1] = &mut self.qbuf;
        Run {
            core: &mut self.core,
            qbuf: [&mut q0[..], &mut q1[..]],
            head: self.head,
            len: self.len,
            mask: self.mask,
            cs: self.cs,
            max_processes: self.max_processes,
            pspace: &mut self.pspace,
            bank: self.bank,
            last_result: &mut self.last_result,
            pspace_size: self.pspace_size,
        }
    }

    /// Keep a view's queue state for the next segment of the round.
    #[inline(always)]
    pub(crate) fn park(&mut self, head: [usize; 2], len: [usize; 2]) {
        self.head = head;
        self.len = len;
    }

    /// Record a round's result for P-space cell 0 of the next round.
    pub(crate) fn finish_round(&mut self, outcome: Outcome, executed: u64) {
        // Counted per round rather than per instruction: the hot loop stays lean.
        self.steps += executed;
        match outcome {
            Outcome::Win(x) => {
                self.last_result[x] = 1;
                self.last_result[1 - x] = 0;
            }
            Outcome::Tie => self.last_result = [2, 2],
        }
    }

    pub fn round(
        &mut self,
        cfg: &Config,
        w: [&Compiled; 2],
        pos: [u32; 2],
        first: usize,
    ) -> Outcome {
        self.start_round(w, pos);
        let (outcome, executed) = self.view().run(cfg.max_cycles as u64 * 2, first);
        self.finish_round(outcome, executed);
        outcome
    }

    pub fn battle(
        &mut self,
        cfg: &Config,
        w: [&Compiled; 2],
        pos: [u32; 2],
        first: usize,
    ) -> Outcome {
        self.begin_match(w[0], w[1]);
        self.round(cfg, w, pos, first)
    }

    /// A match like `pmars -r rounds -F seed+separation` (see `Mars::play`).
    pub fn play(
        &mut self,
        cfg: &Config,
        a: &Compiled,
        b: &Compiled,
        rounds: u32,
        seed: i32,
    ) -> Score {
        let sep = cfg.min_distance;
        let positions = cfg.core_size + 1 - 2 * sep;
        let mut seed = seed;
        let mut score = Score::default();
        self.begin_match(a, b);
        for r in 0..rounds {
            let pos = sep + (seed.rem_euclid(positions as i32)) as u32;
            seed = pmars_rng(seed);
            match self.round(cfg, [a, b], [0, pos], (r % 2) as usize) {
                Outcome::Win(0) => score.w1 += 1,
                Outcome::Win(_) => score.w2 += 1,
                Outcome::Tie => score.ties += 1,
            }
        }
        score
    }
}

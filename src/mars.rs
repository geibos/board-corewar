//! MARS: the Memory Array Redcode Simulator, following the reference
//! emulator in the ICWS'94 draft. This is the plain version, written for
//! being checked against pMARS, not for speed.
//!
//! Every step: copy the instruction under the program counter; evaluate the
//! A operand (predecrement, then read the two numbers of the cell it points
//! to, then postincrement), then the B operand the same way; execute.
//!
//! Where ICWS'94 and pMARS disagree, this follows pMARS, because pMARS is
//! what every hill and every archived warrior was tested on (checked by
//! tests/pmars_diff.rs, read from pMARS 0.9.2 sim.c):
//! * an immediate operand's numbers are those of the instruction as fetched,
//!   before the other operand's pre/postincrements touched it;
//! * MOV.I copies the cell the A operand points to *as it is at execution*
//!   (opcode, modifier, modes) and then puts the A snapshot's numbers in it;
//! * SEQ.I / SNE.I compare opcode, modifier and modes of the two cells as
//!   they are at execution, and the numbers from the snapshots.

use crate::asm::Config;
use crate::redcode::{Instruction, Mode, Modifier, Opcode, Warrior};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Index of the surviving warrior.
    Win(usize),
    Tie,
}

pub struct Mars {
    cs: u32,
    max_processes: usize,
    pub core: Vec<Instruction>,
    queues: Vec<VecDeque<u32>>,
    /// P-space banks; `bank[w]` is the bank warrior w uses (PIN shares one).
    pspace: Vec<Vec<u32>>,
    bank: Vec<usize>,
    /// P-space cell 0 of each warrior: the result of its previous round.
    last_result: Vec<u32>,
    pspace_size: u32,
    /// Instructions executed since creation; for benchmarks.
    pub steps: u64,
}

impl Mars {
    pub fn new(cfg: &Config, warriors: usize) -> Mars {
        Mars {
            cs: cfg.core_size,
            max_processes: cfg.max_processes as usize,
            core: vec![Instruction::EMPTY; cfg.core_size as usize],
            queues: vec![VecDeque::new(); warriors],
            pspace: vec![vec![0; cfg.pspace_size() as usize]; warriors],
            bank: (0..warriors).collect(),
            last_result: vec![cfg.core_size - 1; warriors],
            pspace_size: cfg.pspace_size(),
            steps: 0,
        }
    }

    /// Fresh P-space for a match, shared between warriors with the same
    /// PIN (pspace_init() in pMARS). Cell 0 starts at CORESIZE-1.
    pub fn begin_match(&mut self, warriors: &[&Warrior]) {
        let mut next = 0usize;
        let mut assigned: Vec<Option<usize>> = vec![None; warriors.len()];
        for i in 0..warriors.len() {
            if assigned[i].is_some() {
                continue;
            }
            assigned[i] = Some(next);
            if let Some(pin) = warriors[i].pin {
                for j in i + 1..warriors.len() {
                    if warriors[j].pin == Some(pin) {
                        assigned[j] = Some(next);
                    }
                }
            }
            next += 1;
        }
        self.bank = assigned.into_iter().map(|b| b.unwrap()).collect();
        self.pspace = vec![vec![0; self.pspace_size as usize]; next];
        self.last_result = vec![self.cs - 1; warriors.len()];
    }

    fn get_pspace(&self, w: usize, idx: u32) -> u32 {
        let i = (idx % self.pspace_size) as usize;
        if i == 0 {
            self.last_result[w]
        } else {
            self.pspace[self.bank[w]][i]
        }
    }

    fn set_pspace(&mut self, w: usize, idx: u32, v: u32) {
        let i = (idx % self.pspace_size) as usize;
        if i == 0 {
            self.last_result[w] = v;
        } else {
            let b = self.bank[w];
            self.pspace[b][i] = v;
        }
    }

    #[inline]
    fn add(&self, x: u32, y: u32) -> u32 {
        let s = x + y;
        if s >= self.cs {
            s - self.cs
        } else {
            s
        }
    }
    #[inline]
    fn inc(&self, x: u32) -> u32 {
        self.add(x, 1)
    }
    #[inline]
    fn dec(&self, x: u32) -> u32 {
        if x == 0 {
            self.cs - 1
        } else {
            x - 1
        }
    }

    /// Clear the core and load each warrior at its position.
    pub fn load(&mut self, warriors: &[&Warrior], positions: &[u32]) {
        self.core.fill(Instruction::EMPTY);
        for q in &mut self.queues {
            q.clear();
        }
        for (w, (war, &pos)) in warriors.iter().zip(positions).enumerate() {
            for (k, ins) in war.code.iter().enumerate() {
                let at = self.add(pos, k as u32 % self.cs) as usize;
                self.core[at] = *ins;
            }
            let start = self.add(pos, war.start);
            self.queues[w].push_back(start);
        }
    }

    pub fn processes(&self, w: usize) -> usize {
        self.queues[w].len()
    }

    /// Evaluate one operand: (pointer offset, address of the cell it points
    /// to, snapshot of that cell's A and B numbers at evaluation time).
    #[inline]
    fn operand(
        &mut self,
        pc: u32,
        ir: &Instruction,
        mode: Mode,
        field: u32,
    ) -> (u32, usize, Instruction) {
        if mode == Mode::Immediate {
            return (0, pc as usize, *ir);
        }
        let mut ptr = field;
        let mut post: Option<(usize, bool)> = None;
        if mode != Mode::Direct {
            let p = self.add(pc, field) as usize;
            match mode {
                Mode::APredec => self.core[p].a = self.dec(self.core[p].a),
                Mode::BPredec => self.core[p].b = self.dec(self.core[p].b),
                _ => {}
            }
            let via_a = matches!(mode, Mode::AIndirect | Mode::APredec | Mode::APostinc);
            ptr = self.add(
                ptr,
                if via_a {
                    self.core[p].a
                } else {
                    self.core[p].b
                },
            );
            match mode {
                Mode::APostinc => post = Some((p, true)),
                Mode::BPostinc => post = Some((p, false)),
                _ => {}
            }
        }
        let at = self.add(pc, ptr) as usize;
        let cell = self.core[at];
        if let Some((p, a)) = post {
            if a {
                self.core[p].a = self.inc(self.core[p].a);
            } else {
                self.core[p].b = self.inc(self.core[p].b);
            }
        }
        (ptr, at, cell)
    }

    /// Execute one instruction of warrior `w`. Returns false if the warrior
    /// has no processes left afterwards.
    pub fn step(&mut self, w: usize) -> bool {
        let pc = match self.queues[w].pop_front() {
            Some(pc) => pc,
            None => return false,
        };
        self.steps += 1;
        let ir = self.core[pc as usize];
        let (rpa, a_at, air) = self.operand(pc, &ir, ir.a_mode, ir.a);
        let (wpb, _, mut bir) = self.operand(pc, &ir, ir.b_mode, ir.b);
        let t = self.add(pc, wpb) as usize;
        let next = self.inc(pc);
        let jump = self.add(pc, rpa);
        let skip = self.add(pc, 2);
        let m = ir.modifier;

        let queue = |s: &mut Self, at: u32| s.queues[w].push_back(at);

        match ir.op {
            Opcode::Dat => {}
            Opcode::Nop => queue(self, next),
            Opcode::Mov => {
                let c = &mut self.core[t];
                match m {
                    Modifier::A => c.a = air.a,
                    Modifier::B => c.b = air.b,
                    Modifier::AB => c.b = air.a,
                    Modifier::BA => c.a = air.b,
                    Modifier::F => {
                        c.a = air.a;
                        c.b = air.b;
                    }
                    Modifier::X => {
                        c.a = air.b;
                        c.b = air.a;
                    }
                    Modifier::I => {}
                }
                if m == Modifier::I {
                    let src = self.core[a_at];
                    self.core[t] = Instruction {
                        a: air.a,
                        b: air.b,
                        ..src
                    };
                }
                queue(self, next);
            }
            Opcode::Add | Opcode::Sub | Opcode::Mul => {
                let cs = self.cs;
                let f = |x: u32, y: u32| -> u32 {
                    match ir.op {
                        Opcode::Add => (x + y) % cs,
                        Opcode::Sub => (x + cs - y) % cs,
                        _ => ((x as u64 * y as u64) % cs as u64) as u32,
                    }
                };
                let c = &mut self.core[t];
                match m {
                    Modifier::A => c.a = f(bir.a, air.a),
                    Modifier::B => c.b = f(bir.b, air.b),
                    Modifier::AB => c.b = f(bir.b, air.a),
                    Modifier::BA => c.a = f(bir.a, air.b),
                    Modifier::F | Modifier::I => {
                        c.a = f(bir.a, air.a);
                        c.b = f(bir.b, air.b);
                    }
                    Modifier::X => {
                        c.a = f(bir.a, air.b);
                        c.b = f(bir.b, air.a);
                    }
                }
                queue(self, next);
            }
            Opcode::Div | Opcode::Mod => {
                let div = ir.op == Opcode::Div;
                let f = |x: u32, y: u32| -> Option<u32> {
                    if y == 0 {
                        None
                    } else if div {
                        Some(x / y)
                    } else {
                        Some(x % y)
                    }
                };
                let c = &mut self.core[t];
                let alive = match m {
                    Modifier::A => f(bir.a, air.a).map(|v| c.a = v).is_some(),
                    Modifier::B => f(bir.b, air.b).map(|v| c.b = v).is_some(),
                    Modifier::AB => f(bir.b, air.a).map(|v| c.b = v).is_some(),
                    Modifier::BA => f(bir.a, air.b).map(|v| c.a = v).is_some(),
                    Modifier::F | Modifier::I => {
                        let x = f(bir.a, air.a).map(|v| c.a = v).is_some();
                        let y = f(bir.b, air.b).map(|v| c.b = v).is_some();
                        x && y
                    }
                    Modifier::X => {
                        let x = f(bir.a, air.b).map(|v| c.a = v).is_some();
                        let y = f(bir.b, air.a).map(|v| c.b = v).is_some();
                        x && y
                    }
                };
                if alive {
                    queue(self, next);
                }
            }
            Opcode::Jmp => queue(self, jump),
            Opcode::Jmz => {
                let z = match m {
                    Modifier::A | Modifier::BA => bir.a == 0,
                    Modifier::B | Modifier::AB => bir.b == 0,
                    _ => bir.a == 0 && bir.b == 0,
                };
                queue(self, if z { jump } else { next });
            }
            Opcode::Jmn => {
                let nz = match m {
                    Modifier::A | Modifier::BA => bir.a != 0,
                    Modifier::B | Modifier::AB => bir.b != 0,
                    _ => bir.a != 0 || bir.b != 0,
                };
                queue(self, if nz { jump } else { next });
            }
            Opcode::Djn => {
                let nz = match m {
                    Modifier::A | Modifier::BA => {
                        self.core[t].a = self.dec(self.core[t].a);
                        bir.a = self.dec(bir.a);
                        bir.a != 0
                    }
                    Modifier::B | Modifier::AB => {
                        self.core[t].b = self.dec(self.core[t].b);
                        bir.b = self.dec(bir.b);
                        bir.b != 0
                    }
                    _ => {
                        self.core[t].a = self.dec(self.core[t].a);
                        self.core[t].b = self.dec(self.core[t].b);
                        bir.a = self.dec(bir.a);
                        bir.b = self.dec(bir.b);
                        bir.a != 0 || bir.b != 0
                    }
                };
                queue(self, if nz { jump } else { next });
            }
            Opcode::Spl => {
                queue(self, next);
                if self.queues[w].len() < self.max_processes {
                    queue(self, jump);
                }
            }
            Opcode::Slt => {
                let lt = match m {
                    Modifier::A => air.a < bir.a,
                    Modifier::B => air.b < bir.b,
                    Modifier::AB => air.a < bir.b,
                    Modifier::BA => air.b < bir.a,
                    Modifier::F | Modifier::I => air.a < bir.a && air.b < bir.b,
                    Modifier::X => air.a < bir.b && air.b < bir.a,
                };
                queue(self, if lt { skip } else { next });
            }
            Opcode::Ldp => {
                let v = match m {
                    Modifier::A | Modifier::AB => self.get_pspace(w, air.a),
                    _ => self.get_pspace(w, air.b),
                };
                match m {
                    Modifier::A | Modifier::BA => self.core[t].a = v,
                    _ => self.core[t].b = v,
                }
                queue(self, next);
            }
            Opcode::Stp => {
                match m {
                    Modifier::A => self.set_pspace(w, bir.a, air.a),
                    Modifier::AB => self.set_pspace(w, bir.b, air.a),
                    Modifier::BA => self.set_pspace(w, bir.a, air.b),
                    _ => self.set_pspace(w, bir.b, air.b),
                }
                queue(self, next);
            }
            Opcode::Cmp | Opcode::Seq | Opcode::Sne => {
                let eq = match m {
                    Modifier::A => air.a == bir.a,
                    Modifier::B => air.b == bir.b,
                    Modifier::AB => air.a == bir.b,
                    Modifier::BA => air.b == bir.a,
                    Modifier::F => air.a == bir.a && air.b == bir.b,
                    Modifier::X => air.a == bir.b && air.b == bir.a,
                    Modifier::I => {
                        let (x, y) = (self.core[a_at], self.core[t]);
                        x.op == y.op
                            && x.modifier == y.modifier
                            && x.a_mode == y.a_mode
                            && x.b_mode == y.b_mode
                            && air.a == bir.a
                            && air.b == bir.b
                    }
                };
                let take = if ir.op == Opcode::Sne { !eq } else { eq };
                queue(self, if take { skip } else { next });
            }
        }
        !self.queues[w].is_empty()
    }

    /// One battle of two warriors as a match of one round: fresh P-space.
    pub fn battle(
        &mut self,
        cfg: &Config,
        warriors: [&Warrior; 2],
        positions: [u32; 2],
        first: usize,
    ) -> Outcome {
        self.begin_match(&warriors);
        self.round(cfg, warriors, positions, first)
    }

    /// One round of a match. P-space carries over from earlier rounds, and
    /// each warrior's cell 0 gets this round's result for the next one (0 if
    /// it died, otherwise the number of survivors). `first` moves first;
    /// each warrior gets up to `max_cycles` instructions, as in pMARS.
    pub fn round(
        &mut self,
        cfg: &Config,
        warriors: [&Warrior; 2],
        positions: [u32; 2],
        first: usize,
    ) -> Outcome {
        self.load(&warriors, &positions);
        let mut steps = cfg.max_cycles as u64 * 2;
        let mut w = first;
        let outcome = loop {
            if steps == 0 {
                break Outcome::Tie;
            }
            if !self.step(w) {
                break Outcome::Win(1 - w);
            }
            w = 1 - w;
            steps -= 1;
        };
        match outcome {
            Outcome::Win(x) => {
                self.last_result[x] = 1;
                self.last_result[1 - x] = 0;
            }
            Outcome::Tie => {
                self.last_result[0] = 2;
                self.last_result[1] = 2;
            }
        }
        outcome
    }
}

/// pMARS's position generator (sim.c `rng`): a Park–Miller "minimal
/// standard" generator, used here so that a match of N rounds places the
/// second warrior the way `pmars -r N` does for the same seed.
pub fn pmars_rng(seed: i32) -> i32 {
    let mut temp: i64 = seed as i64;
    temp = 16807 * (temp % 127773) - 2836 * (temp / 127773);
    if temp < 0 {
        temp += 2147483647;
    }
    temp as i32
}

/// Score of a match: wins of the first warrior, wins of the second, ties.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Score {
    pub w1: u32,
    pub w2: u32,
    pub ties: u32,
}

impl Mars {
    /// A match of `rounds` battles, like `pmars -r rounds`: the second
    /// warrior's position is `separation + seed % positions` for a seed that
    /// advances each round, and the warrior that moves first alternates.
    pub fn play(
        &mut self,
        cfg: &Config,
        a: &Warrior,
        b: &Warrior,
        rounds: u32,
        seed: i32,
    ) -> Score {
        let sep = cfg.min_distance;
        let positions = cfg.core_size + 1 - 2 * sep;
        let mut seed = seed;
        let mut score = Score::default();
        self.begin_match(&[a, b]);
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

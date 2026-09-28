//! cw in a web page: the plain engine (src/mars.rs) and the assembler
//! compiled to WebAssembly, so that a replay in a browser is the round cw
//! plays, instruction for instruction, P-space included.
//!
//! No bindings generator: a few C functions over one session.
//!
//! * Text goes in through `cw_input(len)`, a buffer for that many UTF-8
//!   bytes; the last call's answer comes back as JSON (`cw_output_ptr`,
//!   `cw_output_len`). Functions return 1 on success, 0 with
//!   `{"error": ...}` in the output.
//! * `cw_params` sets the rules (the match's rounds among them: a source
//!   may read ROUNDS); `cw_assemble(slot)` assembles the input as
//!   warrior 0 (pMARS's first, see `Config::first_warrior`) or warrior 1.
//! * `cw_match(rounds, seed)`: every round of the match summarised, as
//!   `cw trace` does.
//! * `cw_record(seed, round)`: the match played up to that round, and that
//!   round recorded with everything a replay draws, as arrays of u32 read in
//!   place: the core as it was loaded (`cw_core_*`), the instructions
//!   (`cw_events_*`) and, to check a replay against, the core as the round
//!   ended (`cw_end_*`).
//!
//! A cell is three words: `op | modifier << 5 | a_mode << 8 | b_mode << 11`,
//! then its A and B numbers (0..CORESIZE). Opcodes, modifiers and modes are
//! numbered in the order src/redcode.rs declares them.
//!
//! An event is one instruction: the word `w | pushed << 1 | written << 3`;
//! the cell it executed, which was the head of warrior w's queue; the
//! `pushed` cells it added to the back of that queue, in order; then, once
//! each and by address, every cell it wrote, as the address and the cell's
//! three words after the instruction.
use corewar::asm::{assemble_full, Config};
use corewar::hill;
use corewar::mars::{pmars_rng, Mars, Observer, Outcome};
use corewar::redcode::{Instruction, Warrior};
use corewar::trace;
use serde_json::{json, Value};
use std::cell::RefCell;

/// The three words of a cell.
pub fn encode(i: &Instruction) -> [u32; 3] {
    [
        i.op as u32 | (i.modifier as u32) << 5 | (i.a_mode as u32) << 8 | (i.b_mode as u32) << 11,
        i.a,
        i.b,
    ]
}

#[derive(Default)]
pub struct Session {
    cfg: Config,
    input: Vec<u8>,
    output: String,
    warriors: [Option<Warrior>; 2],
    pub core: Vec<u32>,
    pub events: Vec<u32>,
    pub end: Vec<u32>,
}

fn diags(list: &[corewar::asm::AsmError]) -> Value {
    list.iter()
        .map(|d| json!({"line": d.line, "msg": d.msg}))
        .collect()
}

impl Session {
    /// pMARS's -s -c -p -l -d and the rounds of a match (a source may read
    /// ROUNDS, so it assembles with them), checked the way pMARS checks them.
    pub fn params(
        &mut self,
        core: u32,
        cycles: u32,
        processes: u32,
        length: u32,
        distance: u32,
        rounds: u32,
    ) -> Result<Value, String> {
        let cfg = Config {
            core_size: core,
            max_cycles: cycles,
            max_processes: processes,
            max_length: length as usize,
            min_distance: distance,
            rounds,
            ..Config::default()
        };
        cfg.check(2)?;
        if rounds == 0 {
            return Err("a match has at least one round".into());
        }
        self.cfg = cfg;
        self.warriors = [None, None];
        Ok(json!({"pspace_size": cfg.pspace_size()}))
    }

    /// Assemble `src` as warrior `slot`: its id on a hill, name, author,
    /// start, P-space number and code (three words an instruction), and the
    /// assembler's warnings; or its errors, and the slot is left empty.
    pub fn assemble(&mut self, slot: usize, src: &str) -> Result<Value, String> {
        if slot > 1 {
            return Err(format!("slot {} is not 0 or 1", slot));
        }
        let cfg = Config {
            first_warrior: slot == 0,
            ..self.cfg
        };
        self.warriors[slot] = None;
        match assemble_full(src, &cfg) {
            Ok(a) => {
                let w = a.warrior;
                let out = json!({
                    "ok": true,
                    "id": hill::warrior_id(src),
                    "name": w.name,
                    "author": w.author,
                    "start": w.start,
                    "pin": w.pin,
                    "code": w.code.iter().flat_map(encode).collect::<Vec<u32>>(),
                    "warnings": diags(&a.warnings),
                });
                self.warriors[slot] = Some(w);
                Ok(out)
            }
            Err(errors) => Ok(json!({"ok": false, "errors": diags(&errors)})),
        }
    }

    fn pair(&self) -> Result<(&Warrior, &Warrior), String> {
        match &self.warriors {
            [Some(a), Some(b)] => Ok((a, b)),
            _ => Err("assemble both warriors first".into()),
        }
    }

    /// The hill's position seed for the match of ids `a` and `b`, `a` the
    /// one that sorts first.
    pub fn seed(&self, a: &str, b: &str) -> u32 {
        hill::seed(a, b, self.positions())
    }

    fn positions(&self) -> u32 {
        self.cfg.core_size + 1 - 2 * self.cfg.min_distance
    }

    /// Every round of a match, like `cw trace`: [first, position, winner
    /// (-1 for a tie), end cycle].
    pub fn play(&self, rounds: u32, seed: u32) -> Result<Value, String> {
        let (a, b) = self.pair()?;
        if rounds == 0 {
            return Err("a match has at least one round".into());
        }
        let t = trace::trace(&self.cfg, a, b, rounds, seed as i32, &[], 1);
        let rows: Vec<[i64; 4]> = t
            .rounds
            .iter()
            .map(|r| {
                [
                    r.first as i64,
                    r.position as i64,
                    r.winner.map_or(-1, |w| w as i64),
                    r.end_cycle as i64,
                ]
            })
            .collect();
        Ok(json!({"score": t.score, "rounds": rows}))
    }

    /// Round `round` (from 1) of the match, played after the rounds before
    /// it so that P-space is what it was, and recorded into `core`, `events`
    /// and `end`. Placed as `cw pair` and `cw trace` place it.
    pub fn record(&mut self, seed: u32, round: u32) -> Result<Value, String> {
        if round == 0 {
            return Err("rounds count from 1".into());
        }
        let cfg = self.cfg;
        let (a, b) = self.pair()?;
        let (a, b) = (a.clone(), b.clone());
        let sep = cfg.min_distance;
        let positions = self.positions() as i32;
        let mut seed = seed as i32;
        let mut mars = Mars::new(&cfg, 2);
        mars.begin_match(&[&a, &b]);
        let pspace = |m: &Mars| -> Vec<Vec<u32>> {
            (0..2)
                .map(|w| (0..cfg.pspace_size()).map(|i| m.pspace(w, i)).collect())
                .collect()
        };
        for r in 0..round {
            let pos = sep + seed.rem_euclid(positions) as u32;
            seed = pmars_rng(seed);
            let first = (r % 2) as usize;
            if r + 1 < round {
                mars.round(&cfg, [&a, &b], [0, pos], first);
                continue;
            }
            let before = pspace(&mars);
            self.events.clear();
            let mut rec = Recorder {
                core: &mut self.core,
                events: &mut self.events,
                head: [0; 2],
                len: [0; 2],
                cells: Vec::new(),
            };
            let outcome = mars.round_observed(&cfg, [&a, &b], [0, pos], first, &mut rec);
            self.end.clear();
            for i in &mars.core {
                self.end.extend(encode(i));
            }
            return Ok(json!({
                "round": round,
                "first": first,
                "position": pos,
                "winner": match outcome { Outcome::Win(w) => Some(w), Outcome::Tie => None },
                "end_cycle": mars.last_round_steps.div_ceil(2),
                "steps": mars.last_round_steps,
                "processes": [mars.processes(0), mars.processes(1)],
                "pspace": before,
                "pspace_after": pspace(&mars),
            }));
        }
        unreachable!("the loop returns on its last round")
    }
}

struct Recorder<'a> {
    core: &'a mut Vec<u32>,
    events: &'a mut Vec<u32>,
    /// The head of each queue, and its length, as of the last instruction.
    head: [u32; 2],
    len: [usize; 2],
    cells: Vec<u32>,
}

impl Observer for Recorder<'_> {
    fn loaded(&mut self, mars: &Mars) {
        self.core.clear();
        for i in &mars.core {
            self.core.extend(encode(i));
        }
        for w in 0..2 {
            self.head[w] = mars.queue(w).next().unwrap_or(0);
            self.len[w] = mars.processes(w);
        }
    }

    fn after_step(&mut self, mars: &Mars, w: usize, written: &[u32], _step: u64) {
        // The instruction took the head of w's queue and added to its back;
        // the other warrior's queue did not move.
        let n = mars.processes(w);
        let pushed = n + 1 - self.len[w];
        self.cells.clear();
        self.cells.extend_from_slice(written);
        self.cells.sort_unstable();
        self.cells.dedup();
        self.events
            .push(w as u32 | (pushed as u32) << 1 | (self.cells.len() as u32) << 3);
        self.events.push(self.head[w]);
        self.events.extend(mars.queue(w).skip(n - pushed));
        for &c in &self.cells {
            self.events.push(c);
            self.events.extend(encode(&mars.core[c as usize]));
        }
        self.head[w] = mars.queue(w).next().unwrap_or(0);
        self.len[w] = n;
    }
}

// ---------------------------------------------------------------------------
// The C functions. One thread, one session.

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::default());
}

fn with<T>(f: impl FnOnce(&mut Session) -> T) -> T {
    SESSION.with(|s| f(&mut s.borrow_mut()))
}

/// Put an answer in the output: 1 and the JSON, or 0 and the error.
fn answer(s: &mut Session, r: Result<Value, String>) -> u32 {
    match r {
        Ok(v) => {
            s.output = v.to_string();
            1
        }
        Err(e) => {
            s.output = json!({ "error": e }).to_string();
            0
        }
    }
}

fn input(s: &Session) -> Result<String, String> {
    String::from_utf8(s.input.clone()).map_err(|_| "the text is not UTF-8".to_string())
}

/// A buffer for `len` bytes of input; valid until the next call.
#[no_mangle]
pub extern "C" fn cw_input(len: usize) -> *mut u8 {
    with(|s| {
        s.input.clear();
        s.input.resize(len, 0);
        s.input.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn cw_output_ptr() -> *const u8 {
    with(|s| s.output.as_ptr())
}

#[no_mangle]
pub extern "C" fn cw_output_len() -> usize {
    with(|s| s.output.len())
}

/// The module's version, which is the engine's.
#[no_mangle]
pub extern "C" fn cw_version() -> u32 {
    with(|s| answer(s, Ok(json!({ "version": env!("CARGO_PKG_VERSION") }))))
}

#[no_mangle]
pub extern "C" fn cw_params(
    core: u32,
    cycles: u32,
    processes: u32,
    length: u32,
    distance: u32,
    rounds: u32,
) -> u32 {
    with(|s| {
        let r = s.params(core, cycles, processes, length, distance, rounds);
        answer(s, r)
    })
}

/// Assemble the input as warrior `slot`. 1 even when the source has
/// errors: `ok` in the output says whether it assembled.
#[no_mangle]
pub extern "C" fn cw_assemble(slot: u32) -> u32 {
    with(|s| {
        let r = input(s).and_then(|src| s.assemble(slot as usize, &src));
        answer(s, r)
    })
}

/// The hill's seed for the input "a:b" (two ids, `a` the one that sorts
/// first).
#[no_mangle]
pub extern "C" fn cw_seed() -> u32 {
    with(|s| {
        let r = input(s).and_then(|t| {
            let (a, b) = t.split_once(':').ok_or("expected a:b")?;
            Ok(json!({ "seed": s.seed(a, b) }))
        });
        answer(s, r)
    })
}

#[no_mangle]
pub extern "C" fn cw_match(rounds: u32, seed: u32) -> u32 {
    with(|s| {
        let r = s.play(rounds, seed);
        answer(s, r)
    })
}

#[no_mangle]
pub extern "C" fn cw_record(seed: u32, round: u32) -> u32 {
    with(|s| {
        let r = s.record(seed, round);
        answer(s, r)
    })
}

#[no_mangle]
pub extern "C" fn cw_core_ptr() -> *const u32 {
    with(|s| s.core.as_ptr())
}

#[no_mangle]
pub extern "C" fn cw_core_len() -> usize {
    with(|s| s.core.len())
}

#[no_mangle]
pub extern "C" fn cw_events_ptr() -> *const u32 {
    with(|s| s.events.as_ptr())
}

#[no_mangle]
pub extern "C" fn cw_events_len() -> usize {
    with(|s| s.events.len())
}

#[no_mangle]
pub extern "C" fn cw_end_ptr() -> *const u32 {
    with(|s| s.end.as_ptr())
}

#[no_mangle]
pub extern "C" fn cw_end_len() -> usize {
    with(|s| s.end.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const DWARF: &str = include_str!("../../testdata/dwarf.red");
    const IMP: &str = include_str!("../../testdata/imp.red");
    /// Plays the dwarf after a round it won or tied, the imp after one it
    /// lost: P-space decides what it does.
    const SWITCH: &str = ";redcode-94
;name switch
;author t
start  LDP.AB #0, sel
       JMZ.B  imp, sel
dwarf  ADD.AB #4, bomb
       MOV.I  bomb, @bomb
       JMP    dwarf
bomb   DAT    #0, #0
imp    MOV.I  0, 1
sel    DAT    #0, #0
";
    /// Splits into many processes and bombs with them.
    const PAPER: &str = ";redcode-94
;name paper
;author t
       SPL    1
       SPL    1
       SPL    1
loop   MOV.I  <-10, {20
       ADD.AB #37, loop
       JMP    loop
";

    fn session(a: &str, b: &str) -> Session {
        let mut s = Session::default();
        for (slot, src) in [a, b].into_iter().enumerate() {
            let v = s.assemble(slot, src).unwrap();
            assert_eq!(v["ok"], true, "{}", v);
        }
        s
    }

    /// The replay a page does: from the loaded core, apply every event,
    /// keeping both queues. Returns the core and the queues at the end.
    fn replay(core: &[u32], events: &[u32], heads: [u32; 2]) -> (Vec<u32>, [VecDeque<u32>; 2]) {
        let mut core = core.to_vec();
        let mut q = [VecDeque::from([heads[0]]), VecDeque::from([heads[1]])];
        let mut i = 0;
        while i < events.len() {
            let h = events[i];
            let (w, pushed, written) = ((h & 1) as usize, (h >> 1 & 3) as usize, (h >> 3) as usize);
            assert_eq!(q[w].pop_front(), Some(events[i + 1]), "the head executes");
            i += 2;
            q[w].extend(&events[i..i + pushed]);
            i += pushed;
            for _ in 0..written {
                let c = events[i] as usize;
                core[3 * c..3 * c + 3].copy_from_slice(&events[i + 1..i + 4]);
                i += 4;
            }
        }
        (core, q)
    }

    fn steps(events: &[u32]) -> u64 {
        let (mut i, mut n) = (0, 0);
        while i < events.len() {
            let h = events[i];
            i += 2 + (h >> 1 & 3) as usize + 4 * (h >> 3) as usize;
            n += 1;
        }
        n
    }

    #[test]
    fn a_recorded_round_is_the_round_trace_plays() {
        for (a, b, seed) in [
            (DWARF, IMP, 7u32),
            (SWITCH, DWARF, 1234),
            (PAPER, SWITCH, 55),
            (IMP, PAPER, 3),
        ] {
            let mut s = session(a, b);
            let rounds = 12;
            let t = trace::trace(
                &s.cfg,
                s.warriors[0].as_ref().unwrap(),
                s.warriors[1].as_ref().unwrap(),
                rounds,
                seed as i32,
                &[],
                1,
            );
            let played = s.play(rounds, seed).unwrap();
            for round in 1..=rounds {
                let r = s.record(seed, round).unwrap();
                let want = &t.rounds[round as usize - 1];
                assert_eq!(r["first"], want.first);
                assert_eq!(r["position"], want.position);
                assert_eq!(r["winner"], json!(want.winner));
                assert_eq!(r["end_cycle"], want.end_cycle);
                assert_eq!(played["rounds"][round as usize - 1][3], want.end_cycle);
                assert_eq!(steps(&s.events), r["steps"].as_u64().unwrap());
            }
        }
    }

    #[test]
    fn events_replay_to_the_core_the_round_ended_with() {
        for (a, b, seed) in [(PAPER, DWARF, 9u32), (SWITCH, PAPER, 77), (DWARF, IMP, 7)] {
            let mut s = session(a, b);
            for round in [1, 2, 5] {
                let r = s.record(seed, round).unwrap();
                let pos = r["position"].as_u64().unwrap() as u32;
                let heads = [
                    s.warriors[0].as_ref().unwrap().start,
                    pos + s.warriors[1].as_ref().unwrap().start,
                ];
                let (core, q) = replay(&s.core, &s.events, heads);
                assert!(core == s.end, "round {}: the replayed core differs", round);
                assert_eq!(
                    [q[0].len(), q[1].len()],
                    [
                        r["processes"][0].as_u64().unwrap() as usize,
                        r["processes"][1].as_u64().unwrap() as usize
                    ]
                );
            }
        }
    }

    #[test]
    fn pspace_carries_the_previous_rounds_result() {
        let mut s = session(SWITCH, DWARF);
        let played = s.play(6, 11).unwrap();
        for round in 2..=6u32 {
            let r = s.record(11, round).unwrap();
            let prev = played["rounds"][round as usize - 2][2].as_i64().unwrap();
            let want = match prev {
                -1 => 2,
                0 => 1,
                _ => 0,
            };
            assert_eq!(r["pspace"][0][0], want, "round {}", round);
        }
    }

    #[test]
    fn assembly_gives_the_hill_id_or_the_errors() {
        let mut s = Session::default();
        let v = s.assemble(0, DWARF).unwrap();
        assert_eq!(v["id"], hill::warrior_id(DWARF));
        assert_eq!(
            v["code"].as_array().unwrap().len(),
            3 * s.warriors[0].as_ref().unwrap().code.len()
        );
        let bad = s
            .assemble(1, ";redcode-94\n;name bad\n MOV 0, 1\n FOO 1\n")
            .unwrap();
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["errors"][0]["line"], 4);
        assert!(s.warriors[1].is_none());
        assert!(s.play(1, 1).is_err(), "a match needs both");
    }

    /// ROUNDS is a constant a source may read: the hill assembles with its
    /// own count of rounds, and so must the module.
    #[test]
    fn assembly_sees_the_rounds() {
        let mut s = Session::default();
        s.params(8000, 80000, 8000, 100, 100, 250).unwrap();
        let v = s
            .assemble(0, ";redcode-94\n;name r\n DAT.F #ROUNDS, #0\n")
            .unwrap();
        assert_eq!(v["code"][1], 250);
    }

    #[test]
    fn the_seed_is_the_hills() {
        let s = Session::default();
        // tests/hill.rs derives the same number from sha256("a:b").
        let (a, b) = ("00d694def09ba498", "310a702461164370");
        assert_eq!(s.seed(a, b), hill::seed(a, b, 8000 + 1 - 200));
    }

    #[test]
    fn the_modules_stack_is_the_assemblers() {
        let cfg = include_str!("../.cargo/config.toml");
        let size = cfg
            .split("-zstack-size=")
            .nth(1)
            .and_then(|t| t.split('"').next())
            .and_then(|n| n.parse::<usize>().ok());
        assert_eq!(size, Some(corewar::asm::ASM_STACK));
    }
}

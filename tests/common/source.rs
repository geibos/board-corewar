//! Random Redcode *sources* for the assembler: every preprocessor feature
//! pMARS has, mixed — EQU (single- and multi-line, chained, forward), labels
//! (backward, forward, several per line, with and without `:`), FOR/ROF with
//! `&` concatenation and nesting, CURLINE, registers with `=`, every
//! expression operator, predefined constants, ORG/END/PIN, `\` continuation
//! lines, `;redcode` framing and `;assert`, true or false.
//!
//! The source is built from a vector of choices, so proptest shrinks a
//! failing program by shrinking the choices: shorter and simpler text.
//!
//! What is deliberately never generated: constructs that are undefined
//! behaviour in pMARS's C code (a lone `&`, `|` or `=` between values,
//! numbers of 20+ digits), because no answer there is "the" pMARS answer.

use proptest::prelude::*;

pub fn source() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u32>(), 8..240).prop_map(|c| Builder::new(c).build())
}

struct Builder {
    c: Vec<u32>,
    i: usize,
    labels: Vec<String>,
    equs: Vec<String>,
    macros: Vec<String>,
    counters: Vec<String>,
}

const OPS: [&str; 19] = [
    "mov", "add", "sub", "mul", "div", "mod", "jmz", "jmn", "djn", "cmp", "slt", "spl", "dat",
    "jmp", "seq", "sne", "nop", "ldp", "stp",
];
const MODS: [&str; 7] = ["a", "b", "ab", "ba", "f", "x", "i"];
const MODES: [&str; 9] = ["", "#", "$", "@", "<", ">", "*", "{", "}"];
const BINOPS: [&str; 13] = [
    "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||",
];
const CONSTS: [&str; 5] = [
    "CORESIZE",
    "MAXLENGTH",
    "MINDISTANCE",
    "PSPACESIZE",
    "ROUNDS",
];

impl Builder {
    fn new(c: Vec<u32>) -> Self {
        Builder {
            c,
            i: 0,
            labels: Vec::new(),
            equs: Vec::new(),
            macros: Vec::new(),
            counters: Vec::new(),
        }
    }

    /// Next choice in 0..n; 0 once the choices run out (the simplest case).
    fn n(&mut self, n: u32) -> u32 {
        let v = self.c.get(self.i).copied().unwrap_or(0);
        self.i += 1;
        if n == 0 {
            0
        } else {
            v % n
        }
    }
    fn p(&mut self, percent: u32) -> bool {
        self.n(100) < percent
    }
    fn done(&self) -> bool {
        self.i >= self.c.len()
    }

    fn label_name(&mut self) -> String {
        format!("lb{}", self.n(6))
    }

    fn atom(&mut self, depth: u32) -> String {
        match self.n(12) {
            0..=3 => self.n(21).to_string(),
            4 => (self.n(9000) + 1000).to_string(),
            // mostly a label that exists somewhere, sometimes any: forward and
            // backward references, and some undefined ones for error parity
            5 if !self.labels.is_empty() && self.p(80) => {
                let k = self.n(self.labels.len() as u32) as usize;
                self.labels[k].clone()
            }
            5 => self.label_name(),
            6 if !self.equs.is_empty() => {
                let k = self.n(self.equs.len() as u32) as usize;
                self.equs[k].clone()
            }
            7 => "CURLINE".into(),
            8 => {
                let k = self.n(CONSTS.len() as u32) as usize;
                CONSTS[k].into()
            }
            9 if depth > 0 => {
                // a register, read or assigned
                let r = ["r", "s", "t"][self.n(3) as usize];
                if self.p(50) {
                    format!("({}={})", r, self.expr(depth - 1))
                } else {
                    r.into()
                }
            }
            10 if !self.counters.is_empty() => {
                let k = self.n(self.counters.len() as u32) as usize;
                self.counters[k].clone()
            }
            11 if depth > 0 => format!("({})", self.expr(depth - 1)),
            _ => self.n(10).to_string(),
        }
    }

    fn expr(&mut self, depth: u32) -> String {
        let mut s = match self.n(6) {
            0 => format!("-{}", self.atom(depth)),
            1 if depth > 0 => format!("!{}", self.atom(depth)),
            _ => self.atom(depth),
        };
        let extra = if depth == 0 { 0 } else { self.n(3) };
        for _ in 0..extra {
            let op = BINOPS[self.n(BINOPS.len() as u32) as usize];
            let sp = if self.p(30) { " " } else { "" };
            s = format!("{}{}{}{}{}", s, sp, op, sp, self.atom(depth - 1));
        }
        s
    }

    fn operand(&mut self) -> String {
        let m = MODES[self.n(MODES.len() as u32) as usize];
        format!("{}{}", m, self.expr(2))
    }

    fn instruction(&mut self) -> String {
        let op = OPS[self.n(OPS.len() as u32) as usize];
        let op = if self.p(20) {
            op.to_uppercase()
        } else {
            op.to_string()
        };
        let m = if self.p(50) {
            format!(".{}", MODS[self.n(7) as usize])
        } else {
            String::new()
        };
        let single_ok = matches!(
            op.to_ascii_lowercase().as_str(),
            "dat" | "jmp" | "spl" | "nop"
        );
        let operands = match self.n(20) {
            0..=5 if single_ok => self.operand(),
            0 => self.operand(),
            1 => String::new(),
            _ => format!("{}, {}", self.operand(), self.operand()),
        };
        let mut s = format!("{}{} {}", op, m, operands);
        if self.p(5) {
            // a continuation line inside the operands
            if let Some(k) = s.find(", ") {
                s = format!("{},\\\n   {}", &s[..k], &s[k + 2..]);
            }
        }
        s
    }

    fn labels_prefix(&mut self) -> String {
        let mut s = String::new();
        for _ in 0..self.n(3) {
            let l = self.label_name();
            self.labels.push(l.clone());
            s.push_str(&l);
            s.push_str(if self.p(40) { ": " } else { " " });
        }
        s
    }

    fn line(&mut self, out: &mut String, depth: u32) {
        match self.n(20) {
            0..=2 if depth < 2 => {
                // FOR block
                let cnt = format!("ct{}", self.n(3));
                let count = if self.p(70) {
                    self.n(4).to_string()
                } else {
                    self.expr(1)
                };
                let pre = if self.p(75) {
                    format!("{} ", cnt)
                } else {
                    String::new()
                };
                out.push_str(&format!("{}{}for {}\n", self.labels_prefix(), pre, count));
                if !pre.is_empty() {
                    self.counters.push(cnt.clone());
                }
                for _ in 0..1 + self.n(3) {
                    if !pre.is_empty() && self.p(40) {
                        let l = format!("fl&{}", cnt);
                        out.push_str(&format!("{} {}\n", l, self.instruction()));
                    } else {
                        self.line(out, depth + 1);
                    }
                }
                if !pre.is_empty() {
                    self.counters.pop();
                }
                out.push_str("rof\n");
            }
            1 if !self.macros.is_empty() => {
                let k = self.n(self.macros.len() as u32) as usize;
                out.push_str(&format!("{}{}\n", self.labels_prefix(), self.macros[k]));
            }
            5 => out.push_str(&format!("org {}\n", self.expr(1))),
            3 if self.p(20) => out.push_str(&format!("pin {}\n", self.n(5))),
            4 => out.push_str(&format!("; plain comment {}\n", self.n(9))),
            _ => {
                let l = self.labels_prefix();
                let ins = self.instruction();
                out.push_str(&format!("{}{}\n", l, ins));
            }
        }
    }

    fn build(mut self) -> String {
        let mut out = String::new();
        if self.p(10) {
            out.push_str("text before the header is ignored\n");
        }
        out.push_str(";redcode-94\n;name gen\n;author prop\n");
        if self.p(80) {
            let a = if self.p(85) {
                "1".to_string()
            } else {
                self.expr(1)
            };
            out.push_str(&format!(";assert {}\n", a));
        }
        for k in 0..self.n(3) {
            let name = format!("eq{}", k);
            let body = self.expr(2);
            out.push_str(&format!("{} equ {}\n", name, body));
            self.equs.push(name);
        }
        if self.p(25) {
            let name = format!("mc{}", self.n(2));
            let a = self.instruction();
            out.push_str(&format!("{} equ {}\n", name, a));
            if self.p(50) {
                let b = self.instruction();
                out.push_str(&format!("   equ {}\n", b));
            }
            self.macros.push(name);
        }
        let lines = 1 + self.n(10);
        for _ in 0..lines {
            if self.done() {
                break;
            }
            self.line(&mut out, 0);
        }
        match self.n(4) {
            0 => out.push_str("end\n"),
            1 => {
                let l = self.label_name();
                out.push_str(&format!("end {}\n", l));
            }
            2 => out.push_str(&format!("end {}\n", self.n(3))),
            _ => {}
        }
        if self.p(10) {
            out.push_str(";redcode\nmov 0, 1 ; a second warrior: ignored\n");
        }
        out
    }
}

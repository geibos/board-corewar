//! Redcode assembler: a port of pMARS 0.9.2 (asm.c, token.c, eval.c).
//!
//! pMARS is the assembler every hill and warrior archive was built with, and
//! "follows the ICWS'94 draft" is not precise enough to agree with it on
//! real warriors: its assembler is a text preprocessor with its own rules.
//! So this is a port of its algorithm, structure for structure, not a fresh
//! parser of the standard:
//!
//! * reading: `\` joins lines, text before the first `;redcode` is dropped
//!   and a second `;redcode` ends the warrior, `;name`/`;author` are read,
//!   `;assert` lines are kept and checked;
//! * pass 1 rewrites the text: `EQU` (single- and multi-line) is textual
//!   substitution, labels already seen become relative numbers, `FOR`/`ROF`
//!   blocks are unrolled with `&counter` concatenation, `CURLINE` becomes the
//!   line number;
//! * pass 2 runs pMARS's line automaton over the rewritten text and
//!   evaluates the fields with pMARS's expression evaluator, including its
//!   26 registers `a`..`z` with `=` assignment and its precedence quirks.
//!
//! Warnings do not stop assembly; errors do. Which is which follows pMARS.

use crate::redcode::{Instruction, Mode, Modifier, Opcode, Warrior};
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub core_size: u32,
    pub max_length: usize,
    pub max_processes: u32,
    pub max_cycles: u32,
    pub min_distance: u32,
    pub warriors: u32,
    pub rounds: u32,
    /// pMARS checks its score formula `(W*W-1)/S` before assembling, which
    /// leaves registers W and S holding the number of warriors for the
    /// *first* warrior it assembles; later ones start with all registers 0.
    /// A warrior that reads W or S without assigning them therefore
    /// assembles differently in pMARS depending on its place on the command
    /// line. `true` assembles like the first (the default), `false` like
    /// the others.
    pub first_warrior: bool,
}

impl Default for Config {
    /// The standard '94 hill: the numbers pMARS uses when given no options.
    fn default() -> Self {
        Config {
            core_size: 8000,
            max_length: 100,
            max_processes: 8000,
            max_cycles: 80000,
            min_distance: 100,
            warriors: 2,
            rounds: 1,
            first_warrior: true,
        }
    }
}

impl Config {
    /// pMARS's default P-space size: the core divided by its largest divisor
    /// not above 16 (500 for the standard core of 8000).
    pub fn pspace_size(&self) -> u32 {
        (1..=16u32)
            .rev()
            .find(|i| self.core_size % i == 0)
            .map(|i| self.core_size / i)
            .unwrap_or(self.core_size)
    }
}

/// One message from the assembler. `line` is the source line (1-based),
/// 0 when the message is about the whole warrior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    pub line: usize,
    pub msg: String,
    pub warning: bool,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.warning { "warning" } else { "error" };
        if self.line == 0 {
            write!(f, "{}: {}", kind, self.msg)
        } else {
            write!(f, "line {}: {}: {}", self.line, kind, self.msg)
        }
    }
}

impl std::error::Error for AsmError {}

/// A successful assembly and the warnings it produced.
#[derive(Debug, Clone)]
pub struct Assembly {
    pub warrior: Warrior,
    pub warnings: Vec<AsmError>,
}

/// Assemble a warrior; the first error, if any.
pub fn assemble(src: &str, cfg: &Config) -> Result<Warrior, AsmError> {
    assemble_full(src, cfg)
        .map(|a| a.warrior)
        .map_err(|mut e| e.remove(0))
}

/// Assemble a warrior, keeping every warning; on failure, every error.
pub fn assemble_full(src: &str, cfg: &Config) -> Result<Assembly, Vec<AsmError>> {
    let mut a = Asm::new(cfg);
    let warrior = a.run(src);
    let (errors, warnings): (Vec<AsmError>, Vec<AsmError>) =
        a.diags.into_iter().partition(|d| !d.warning);
    if errors.is_empty() {
        Ok(Assembly { warrior, warnings })
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// token.c

const MAXALLCHAR: usize = 256;
const GRPMAX: usize = 7;
const MAXINSTR: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tt {
    None,
    Numb,
    Char,
    Expr,
    Addr,
    Apnd,
    Comm,
    Fsep,
    Modf,
    Misc,
}

const EXPR_SYM: &[u8] = b"()/+-%!=";
const ADDR_SYM: &[u8] = b"#$@<>*{}";

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

/// get_token(): the next token starting at `*i`, after whitespace.
fn get_token(b: &[u8], i: &mut usize) -> (Tt, String) {
    let mut src = *i;
    while is_space(at(b, src)) {
        src += 1;
    }
    let mut tok = Vec::new();
    let tt;
    let ch = at(b, src);
    if ch == 0 {
        tt = Tt::None;
    } else if ch.is_ascii_digit() {
        while at(b, src).is_ascii_digit() {
            tok.push(b[src]);
            src += 1;
        }
        tt = Tt::Numb;
    } else if ch.is_ascii_alphabetic() || ch == b'_' {
        while {
            let c = at(b, src);
            c != 0 && (c.is_ascii_alphanumeric() || c == b'_')
        } {
            tok.push(b[src]);
            src += 1;
        }
        tt = Tt::Char;
    } else {
        if EXPR_SYM.contains(&ch) {
            tt = Tt::Expr;
        } else if ADDR_SYM.contains(&ch) {
            tt = Tt::Addr;
        } else if ch == b'&' {
            if at(b, src + 1) == b'&' {
                tok.push(b[src]);
                src += 1;
                tt = Tt::Expr;
            } else {
                tt = Tt::Apnd;
            }
        } else if ch == b';' {
            tt = Tt::Comm;
        } else if ch == b',' {
            tt = Tt::Fsep;
        } else if ch == b'.' {
            tt = Tt::Modf;
        } else if ch == b'|' && at(b, src + 1) == b'|' {
            tok.push(b[src]);
            src += 1;
            tt = Tt::Expr;
        } else {
            tt = Tt::Misc;
        }
        tok.push(b[src]);
        src += 1;
    }
    *i = src;
    (tt, String::from_utf8_lossy(&tok).into_owned())
}

// ---------------------------------------------------------------------------
// eval.c — ported literally, quirks included: operators are encoded as in C
// (EQUAL is 0, the same value as "no saved operator"), unary minus binds to
// the next value only, and a..z are registers that `=` assigns.

const EQUAL: u8 = 0;
const NEQU: u8 = 1;
const GTE: u8 = 2;
const LTE: u8 = 3;
const AND: u8 = 4;
const OR: u8 = 5;
const IDENT: u8 = 6;

const OK_EXPR: i32 = 0;
const OVERFLOW: i32 = 1;
const BAD_EXPR: i32 = -1;
const DIV_ZERO: i32 = -2;

fn precedence(op: u8) -> i32 {
    match op {
        b'*' | b'/' | b'%' => 5,
        b'+' | b'-' => 4,
        b'>' | b'<' | EQUAL | NEQU | GTE | LTE => 3,
        AND => 2,
        OR => 1,
        _ => 0,
    }
}

struct Eval<'a> {
    s: &'a [u8],
    regs: &'a mut [i64; 26],
    save_oper: &'a mut u8,
    err: i32,
}

impl Eval<'_> {
    fn c(&self, i: usize) -> u8 {
        at(self.s, i)
    }
    fn skip(&self, mut i: usize) -> usize {
        while is_space(self.c(i)) {
            i += 1;
        }
        i
    }

    fn calc(&mut self, x: i64, y: i64, op: u8) -> i64 {
        match op {
            b'+' => {
                if self.err == OK_EXPR
                    && (if x > 0 {
                        y > 0 && x > i64::MAX - y
                    } else {
                        y < 0 && x < i64::MIN - y
                    })
                {
                    self.err = OVERFLOW;
                }
                x.wrapping_add(y)
            }
            b'-' => {
                if self.err == OK_EXPR
                    && (if x > 0 {
                        y < 0 && x > i64::MAX.wrapping_add(y)
                    } else {
                        y > 0 && x < i64::MIN.wrapping_add(y)
                    })
                {
                    self.err = OVERFLOW;
                }
                x.wrapping_sub(y)
            }
            b'/' => {
                if y == 0 {
                    self.err = DIV_ZERO;
                    0
                } else {
                    x.wrapping_div(y)
                }
            }
            b'*' => {
                if self.err == OK_EXPR
                    && x != 0
                    && y != 0
                    && x != -1
                    && y != -1
                    && (if (x > 0) == (y > 0) {
                        i64::MAX / y / x == 0
                    } else {
                        i64::MIN / y / x == 0
                    })
                {
                    self.err = OVERFLOW;
                }
                x.wrapping_mul(y)
            }
            b'%' => {
                if y == 0 {
                    self.err = DIV_ZERO;
                    0
                } else {
                    x.wrapping_rem(y)
                }
            }
            AND => (x != 0 && y != 0) as i64,
            OR => (x != 0 || y != 0) as i64,
            EQUAL => (x == y) as i64,
            NEQU => (x != y) as i64,
            b'<' => (x < y) as i64,
            b'>' => (x > y) as i64,
            LTE => (x <= y) as i64,
            GTE => (x >= y) as i64,
            IDENT => y,
            _ => {
                self.err = BAD_EXPR;
                0
            }
        }
    }

    /// getop(); `None` where C would leave the operator uninitialized.
    fn getop(&mut self, mut i: usize) -> (usize, Option<u8>) {
        let ch = self.c(i);
        i += 1;
        let op = match ch {
            b'&' => {
                let n = self.c(i);
                i += 1;
                (n == b'&').then_some(AND)
            }
            b'|' => {
                let n = self.c(i);
                i += 1;
                (n == b'|').then_some(OR)
            }
            b'=' => {
                let n = self.c(i);
                i += 1;
                (n == b'=').then_some(EQUAL)
            }
            b'!' => {
                let n = self.c(i);
                i += 1;
                (n == b'=').then_some(NEQU)
            }
            b'<' => {
                if self.c(i) == b'=' {
                    i += 1;
                    Some(LTE)
                } else {
                    Some(b'<')
                }
            }
            b'>' => {
                if self.c(i) == b'=' {
                    i += 1;
                    Some(GTE)
                } else {
                    Some(b'>')
                }
            }
            other => Some(other),
        };
        (i, op)
    }

    fn getreg(&mut self, i: usize, reg: usize) -> (usize, i64) {
        let i = self.skip(i);
        if self.c(i) == b'=' && self.c(i + 1) != b'=' {
            let (j, v) = self.eval(-1, 0, IDENT, i + 1);
            self.regs[reg] = v;
            (j, v)
        } else {
            (i, self.regs[reg])
        }
    }

    fn getval(&mut self, i: usize) -> (usize, i64) {
        let mut i = self.skip(i);
        let c = self.c(i);
        if c == b'(' {
            let (j, v) = self.eval(-1, 0, IDENT, i + 1);
            if self.c(j) != b')' {
                self.err = BAD_EXPR;
            }
            return (j + 1, v);
        }
        if c == b'-' {
            let (j, v) = self.getval(i + 1);
            return (j, v.wrapping_mul(-1));
        }
        if c == b'!' {
            let (j, v) = self.getval(i + 1);
            return (j, (v == 0) as i64);
        }
        if c == b'+' {
            return self.getval(i + 1);
        }
        let up = c.to_ascii_uppercase();
        if up.is_ascii_uppercase() {
            return self.getreg(i + 1, (up - b'A') as usize);
        }
        let st = i;
        while self.c(i).is_ascii_digit() {
            i += 1;
        }
        if i == st {
            self.err = BAD_EXPR;
            return (i, 0);
        }
        // sscanf("%ld") on at most 19 digits; longer is undefined in C.
        let digits = std::str::from_utf8(&self.s[st..i]).unwrap_or("0");
        let v = digits.parse::<i64>().unwrap_or_else(|_| {
            self.err = BAD_EXPR;
            0
        });
        (i, v)
    }

    fn eval(&mut self, prev_prec: i32, val1: i64, oper1: u8, i: usize) -> (usize, i64) {
        let (i, val2) = self.getval(i);
        let i = self.skip(i);
        let c = self.c(i);
        if c == b')' || c == 0 {
            return (i, self.calc(val1, val2, oper1));
        }
        let (i, oper2) = self.getop(i);
        let Some(oper2) = oper2 else {
            self.err = BAD_EXPR;
            return (self.s.len(), 0);
        };
        *self.save_oper = 0;
        let (prec1, prec2) = (precedence(oper1), precedence(oper2));
        if prec1 >= prec2 {
            if prec2 >= prev_prec || prec1 <= prev_prec {
                let v = self.calc(val1, val2, oper1);
                self.eval(prec1, v, oper2, i)
            } else {
                let r = self.calc(val1, val2, oper1);
                *self.save_oper = oper2;
                (i, r)
            }
        } else {
            let (i, r2) = self.eval(prec1, val2, oper2, i);
            let mut r = self.calc(val1, r2, oper1);
            let mut i = i;
            let so = *self.save_oper;
            if so != 0 && precedence(so) >= prev_prec {
                let (j, v) = self.eval(prec2, r, so, i);
                i = j;
                r = v;
                *self.save_oper = 0;
            }
            (i, r)
        }
    }
}

/// eval_expr(): (status, value). Status is OK_EXPR, OVERFLOW (a warning),
/// BAD_EXPR or DIV_ZERO.
fn eval_expr(expr: &str, regs: &mut [i64; 26], save_oper: &mut u8) -> (i32, i64) {
    let mut e = Eval {
        s: expr.as_bytes(),
        regs,
        save_oper,
        err: OK_EXPR,
    };
    let (i, v) = e.eval(-1, 0, IDENT, 0);
    if at(e.s, i) != 0 {
        e.err = BAD_EXPR;
    }
    (e.err, v)
}

// ---------------------------------------------------------------------------
// asm.c

/// opname[]: the order matters, pseudo-opcodes follow the real ones.
const OPNAME: [&str; 22] = [
    "MOV", "ADD", "SUB", "MUL", "DIV", "MOD", "JMZ", "JMN", "DJN", "CMP", "SLT", "SPL", "DAT",
    "JMP", "SEQ", "SNE", "NOP", "LDP", "STP", "ORG", "END", "PIN",
];
const OPNUM: usize = 19;
const ORGOP: usize = 19;
const ENDOP: usize = 20;
const PINOP: usize = 21;
const EQUOP: usize = 22;
const MODNAME: [&str; 7] = ["A", "B", "AB", "BA", "F", "X", "I"];

fn opname_index(up: &str) -> usize {
    OPNAME.iter().position(|o| *o == up).unwrap_or(EQUOP)
}

fn opcode_of(i: usize) -> Opcode {
    [
        Opcode::Mov,
        Opcode::Add,
        Opcode::Sub,
        Opcode::Mul,
        Opcode::Div,
        Opcode::Mod,
        Opcode::Jmz,
        Opcode::Jmn,
        Opcode::Djn,
        Opcode::Cmp,
        Opcode::Slt,
        Opcode::Spl,
        Opcode::Dat,
        Opcode::Jmp,
        Opcode::Seq,
        Opcode::Sne,
        Opcode::Nop,
        Opcode::Ldp,
        Opcode::Stp,
    ][i]
}

fn mode_of(c: u8) -> Mode {
    Mode::from_char(c as char).unwrap_or(Mode::Direct)
}

// trav2() states
const SNIL: i32 = 0;
const SLBL: i32 = 1;
const SVAL: i32 = 2;
const SCOM: i32 = 3;
const SPSE: i32 = 4;
const SFOR: i32 = 5;
const SROF: i32 = 6;
const SERR: i32 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    Op,
    ModAddrExp,
    Modf,
    AddrExpA,
    ExpFs,
    AddrExpB,
    Expr,
}

#[derive(Clone, Debug)]
struct Line {
    text: String,
    loc: usize,
}

#[derive(Clone, Debug)]
enum RefKind {
    /// EQU text: an index into `lists`.
    Text(usize),
    /// A FOR counter.
    Stack(u32),
    /// A label: the instruction number it names.
    Label(u32),
}

#[derive(Clone, Debug)]
struct Ref {
    names: Vec<String>,
    kind: RefKind,
    visit: bool,
}

#[derive(Clone, Copy, Debug)]
enum E {
    Buf,
    Tok,
    Syn,
    Snf,
    Nop,
    Evl,
    Exp,
    Rec,
    Ann,
    Lin,
    App,
    Zln,
    Idn,
    Rof,
    For,
    Grp,
    Chk,
    Nas,
    Bas,
    Exx,
    Cat,
    Dlb,
    Ofs,
    Doe,
    Div,
    Ofl,
    Misc,
}

impl E {
    fn warning(self) -> bool {
        matches!(
            self,
            E::Ann | E::Rof | E::Zln | E::Dlb | E::Ofl | E::Nas | E::Ofs | E::Doe | E::Bas | E::Idn
        )
    }
    fn text(self) -> &'static str {
        match self {
            E::Buf => "line too long",
            E::Tok => "unexpected token",
            E::Syn => "missing operand",
            E::Snf => "unknown symbol",
            E::Nop => "this instruction needs two operands",
            E::Evl => "bad expression",
            E::Exp => "expected an opcode or modifier",
            E::Rec => "recursive EQU",
            E::Ann => "EQU without a name",
            E::Lin => "too many instructions",
            E::App => "not allowed here",
            E::Zln => "no instructions",
            E::Idn => "label defined twice; the line is dropped",
            E::Rof => "FOR without ROF",
            E::For => "ROF without FOR",
            E::Grp => "too many labels on one line",
            E::Chk => "assertion failed",
            E::Nas => "missing ;assert",
            E::Bas => "bad ;assert expression",
            E::Exx => "program too long",
            E::Cat => "& needs a FOR counter",
            E::Dlb => "labels with no instruction after them are dropped",
            E::Ofs => "start is outside the program",
            E::Doe => "END start ignored: ORG already set it",
            E::Div => "division by zero",
            E::Ofl => "arithmetic overflow",
            E::Misc => "CURLINE cannot be a label",
        }
    }
}

struct Asm<'c> {
    cfg: &'c Config,
    lists: Vec<Vec<Line>>,
    refs: Vec<Ref>,
    symtbl: Vec<String>,
    statefine: u32,
    line: u32,
    vcont: bool,
    aline: Option<(usize, usize)>,
    out: Vec<Line>,
    diags: Vec<AsmError>,
    errnum: usize,
    noassert: bool,
    regs: [i64; 26],
    save_oper: u8,
    // pass 2
    opcode: usize,
    modifier: Option<usize>,
    filled: bool,
    laststate: St,
    a_expr: String,
    b_expr: String,
    a_mode: Mode,
    b_mode: Mode,
    line_err: bool,
    name: String,
    author: String,
    offset: i64,
    pin: Option<i64>,
}

fn concat(dest: &mut String, s: &str) -> bool {
    if dest.len() + s.len() < MAXALLCHAR {
        dest.push_str(s);
        true
    } else {
        false
    }
}

impl<'c> Asm<'c> {
    fn new(cfg: &'c Config) -> Self {
        Asm {
            cfg,
            lists: vec![Vec::new()],
            refs: Vec::new(),
            symtbl: Vec::new(),
            statefine: 0,
            line: 0,
            vcont: true,
            aline: None,
            out: Vec::new(),
            diags: Vec::new(),
            errnum: 0,
            noassert: true,
            regs: [0; 26],
            save_oper: 0,
            opcode: 0,
            modifier: None,
            filled: false,
            laststate: St::Op,
            a_expr: String::new(),
            b_expr: String::new(),
            a_mode: Mode::Direct,
            b_mode: Mode::Direct,
            line_err: false,
            name: "Unknown".into(),
            author: "Anonymous".into(),
            offset: 0,
            pin: None,
        }
    }

    fn err(&mut self, e: E) {
        let loc = self
            .aline
            .and_then(|(l, i)| self.lists.get(l).and_then(|v| v.get(i)))
            .map(|x| x.loc)
            .unwrap_or(0);
        self.err_at(e, loc);
    }

    fn err_at(&mut self, e: E, loc: usize) {
        self.line_err = true;
        if !e.warning() {
            self.errnum += 1;
        }
        self.diags.push(AsmError {
            line: loc,
            msg: e.text().into(),
            warning: e.warning(),
        });
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        (0..self.refs.len())
            .rev()
            .find(|&i| self.refs[i].names.iter().any(|n| n == name))
    }

    fn cur_text(&self) -> Option<String> {
        self.aline.map(|(l, i)| self.lists[l][i].text.clone())
    }

    fn next_line(&self) -> Option<(usize, usize)> {
        let (l, i) = self.aline?;
        (i + 1 < self.lists[l].len()).then_some((l, i + 1))
    }

    fn addline(&mut self, text: &str) {
        let loc = self.aline.map(|(l, i)| self.lists[l][i].loc).unwrap_or(0);
        self.out.push(Line {
            text: text.to_string(),
            loc,
        });
    }

    fn addpredefs(&mut self) {
        let c = self.cfg;
        let defs = [
            ("CORESIZE", c.core_size as u64),
            ("MAXPROCESSES", c.max_processes as u64),
            ("MAXCYCLES", c.max_cycles as u64),
            ("MAXLENGTH", c.max_length as u64),
            ("MINDISTANCE", c.min_distance as u64),
            ("VERSION", 92),
            ("WARRIORS", c.warriors as u64),
            ("ROUNDS", c.rounds as u64),
            ("PSPACESIZE", c.pspace_size() as u64),
        ];
        for (n, v) in defs {
            self.lists.push(vec![Line {
                text: v.to_string(),
                loc: 0,
            }]);
            let id = self.lists.len() - 1;
            self.refs.push(Ref {
                names: vec![n.into()],
                kind: RefKind::Text(id),
                visit: false,
            });
        }
    }

    fn eval(&mut self, expr: &str) -> (i32, i64) {
        eval_expr(expr, &mut self.regs, &mut self.save_oper)
    }

    // ---- reading -----------------------------------------------------------

    /// The file-reading loop of assemble(): fgets() in 256-byte buffers,
    /// `\` continuation, `;redcode` framing, global comment switches.
    fn read(&mut self, src: &str) {
        let data = src.as_bytes();
        let mut pos = 0usize;
        let mut lines = 0usize;
        let mut pstart = 0;
        let mut cont = true;
        while cont {
            let mut buf: Vec<u8> = Vec::new();
            loop {
                // fgets(buf + i, MAXALLCHAR - i): up to 255 - i bytes, through '\n'.
                let room = MAXALLCHAR - 1 - buf.len();
                if pos >= data.len() {
                    cont = false;
                    break;
                }
                let mut chunk = Vec::new();
                while chunk.len() < room && pos < data.len() {
                    let c = data[pos];
                    pos += 1;
                    chunk.push(c);
                    if c == b'\n' {
                        break;
                    }
                }
                if let Some(cut) = chunk.iter().position(|&c| c == b'\n' || c == b'\r') {
                    chunk.truncate(cut);
                }
                buf.extend_from_slice(&chunk);
                if buf.last() == Some(&b'\\') {
                    buf.pop();
                    continue;
                }
                break;
            }
            lines += 1;
            let text = String::from_utf8_lossy(&buf).into_owned();
            let b = text.as_bytes();
            let mut i = 0;
            let (tt, _) = get_token(b, &mut i);
            match tt {
                Tt::Comm => {
                    if self.globalswitch(&text, i, lines) {
                        match pstart {
                            0 => {
                                self.lists[0].clear();
                                pstart = 1;
                            }
                            _ => cont = false,
                        }
                    }
                }
                Tt::None => {}
                _ => {
                    let code = text.split(';').next().unwrap_or("").to_string();
                    self.lists[0].push(Line {
                        text: code,
                        loc: lines,
                    });
                }
            }
        }
    }

    /// Returns true for `;redcode`.
    fn globalswitch(&mut self, s: &str, idx: usize, loc: usize) -> bool {
        let b = s.as_bytes();
        let mut i = idx;
        let (_, tok) = get_token(b, &mut i);
        let up = tok.to_ascii_uppercase();
        if up == "REDCODE" && i == idx + 7 {
            return true;
        }
        while is_space(at(b, i)) {
            i += 1;
        }
        let rest = &s[i.min(s.len())..];
        match up.as_str() {
            "NAME" => {
                self.name = if rest.is_empty() {
                    "Unknown".into()
                } else {
                    rest.into()
                }
            }
            "AUTHOR" => {
                self.author = if rest.is_empty() {
                    "Anonymous".into()
                } else {
                    rest.into()
                }
            }
            "DATE" | "VERSION" => {}
            "ASSERT" => {
                // nocmnt(str + i): keep the first ';', drop a second comment.
                let tail = rest.split(';').next().unwrap_or("");
                let text = format!("{}{}", &s[..i.min(s.len())], tail);
                self.lists[0].push(Line { text, loc });
            }
            _ => {}
        }
        false
    }

    // ---- pass 1 --------------------------------------------------------------

    fn expand(&mut self) {
        self.vcont = true;
        self.statefine = 0;
        self.aline = if self.lists[0].is_empty() {
            None
        } else {
            Some((0, 0))
        };
        while let (Some(_), true) = (self.aline, self.vcont) {
            let text = self.cur_text().unwrap();
            let mut d = String::new();
            match self.trav2(&text, &mut d, SNIL) {
                SCOM => self.addline(&d),
                SROF => self.err(E::For),
                _ => {}
            }
            self.aline = self.next_line();
        }
    }

    fn blkfor(&mut self, expr: &str, dest: &mut String) -> i32 {
        let group: Vec<String> = if let Some(counter) = self.symtbl.last().cloned() {
            if self.symtbl.len() > 1 {
                let rest: Vec<String> = self.symtbl[..self.symtbl.len() - 1].to_vec();
                self.refs.push(Ref {
                    names: rest,
                    kind: RefKind::Label(self.line),
                    visit: false,
                });
            }
            self.symtbl.clear();
            vec![counter]
        } else {
            Vec::new()
        };
        dest.clear();
        self.trav2(expr, dest, SVAL);
        let (st, result) = self.eval(dest);
        if st < OK_EXPR {
            self.err(if st == DIV_ZERO { E::Div } else { E::Evl });
        } else if result <= 0 {
            if st == OVERFLOW {
                self.err(E::Ofl);
            }
            self.statefine += 1;
        } else {
            if st == OVERFLOW {
                self.err(E::Ofl);
            }
            self.refs.push(Ref {
                names: group,
                kind: RefKind::Stack(1),
                visit: true,
            });
            let me = self.refs.len() - 1;
            let limit = result as u16 as u32;
            let cline = self.aline;
            let mut n = 1u32;
            while self.vcont && n <= limit && self.aline.is_some() {
                self.refs[me].kind = RefKind::Stack(n);
                self.aline = cline;
                while self.vcont {
                    if let Some(nx) = self.next_line() {
                        self.aline = Some(nx);
                        dest.clear();
                        let text = self.cur_text().unwrap();
                        let r = self.trav2(&text, dest, SNIL);
                        if r == SROF {
                            break;
                        } else if r == SCOM {
                            let d = dest.clone();
                            self.addline(&d);
                        }
                    } else {
                        self.err(E::Rof);
                        self.vcont = false;
                    }
                }
                // Guard against runaway expansion (pMARS would exhaust memory).
                if self.out.len() > 100_000 {
                    self.err(E::Exx);
                    self.vcont = false;
                }
                n += 1;
            }
            // Remove the most recent FOR counter.
            if let Some(k) = (0..self.refs.len())
                .rev()
                .find(|&k| matches!(self.refs[k].kind, RefKind::Stack(_)))
            {
                self.refs.remove(k);
            }
        }
        SFOR
    }

    fn equtbl(&mut self, expr: &str) -> i32 {
        if !self.symtbl.is_empty() {
            let loc = self.aline.map(|(l, i)| self.lists[l][i].loc).unwrap_or(0);
            let mut body = vec![Line {
                text: expr.to_string(),
                loc,
            }];
            while let Some(nx) = self.next_line() {
                let t = self.lists[nx.0][nx.1].text.clone();
                let mut i = 0;
                let (_, tok) = get_token(t.as_bytes(), &mut i);
                if tok.eq_ignore_ascii_case("EQU") {
                    self.aline = Some(nx);
                    body.push(Line {
                        text: t[i..].to_string(),
                        loc: self.lists[nx.0][nx.1].loc,
                    });
                } else {
                    break;
                }
            }
            self.lists.push(body);
            let id = self.lists.len() - 1;
            let names = std::mem::take(&mut self.symtbl);
            self.refs.push(Ref {
                names,
                kind: RefKind::Text(id),
                visit: false,
            });
        } else {
            self.err(E::Ann);
        }
        SVAL
    }

    fn equsub(&mut self, expr: &str, dest: &mut String, wdecl: i32, ri: usize) -> i32 {
        let RefKind::Text(list) = self.refs[ri].kind else {
            unreachable!()
        };
        self.refs[ri].visit = true;
        let saved = self.aline;
        self.aline = Some((list, 0));
        let text = self.cur_text().unwrap();
        let mut w = self.trav2(&text, dest, wdecl);
        while let (Some(nx), true) = (self.next_line(), self.vcont) {
            if self.statefine == 0 && w == SCOM {
                let d = dest.clone();
                self.addline(&d);
            }
            self.aline = Some(nx);
            dest.clear();
            let text = self.cur_text().unwrap();
            w = self.trav2(&text, dest, SNIL);
        }
        self.aline = saved;
        if is_space(at(expr.as_bytes(), 0)) {
            concat(dest, " ");
        }
        self.refs[ri].visit = false;
        self.trav2(expr, dest, w)
    }

    /// trav2(): rewrite one line (or the rest of one) into `dest`.
    fn trav2(&mut self, buffer: &str, dest: &mut String, wdecl: i32) -> i32 {
        let b = buffer.as_bytes();
        let mut idxp = 0usize;
        let (tt, token) = get_token(b, &mut idxp);
        match tt {
            Tt::None => wdecl,
            Tt::Comm => {
                if wdecl == SNIL && self.statefine == 0 {
                    let mut idx = idxp;
                    let (_, t2) = get_token(b, &mut idx);
                    if t2.eq_ignore_ascii_case("ASSERT") {
                        self.trav2(&buffer[idx..], dest, SVAL);
                        let (st, v) = self.eval(dest);
                        if st < OK_EXPR {
                            self.err(E::Bas);
                        } else {
                            if st == OVERFLOW {
                                self.err(E::Ofl);
                            }
                            self.noassert = false;
                            if v == 0 {
                                self.err(E::Chk);
                            }
                        }
                    }
                }
                wdecl
            }
            Tt::Char => {
                let up = token.to_ascii_uppercase();
                let rest = &buffer[idxp..];
                if up == "ROF" {
                    if self.statefine > 0 {
                        self.statefine -= 1;
                    } else if wdecl <= SLBL {
                        return SROF;
                    } else {
                        self.err(E::App);
                    }
                } else if up == "FOR" {
                    if self.statefine > 0 {
                        self.statefine += 1;
                    } else if wdecl <= SLBL {
                        return self.blkfor(rest, dest);
                    } else {
                        self.err(E::App);
                    }
                } else if token == "CURLINE" {
                    if self.statefine > 0 {
                        return SNIL;
                    } else if wdecl > SLBL {
                        let mut s = self.line.to_string();
                        if is_space(at(b, idxp)) {
                            s.push(' ');
                        }
                        if concat(dest, &s) {
                            return self.trav2(rest, dest, wdecl);
                        }
                        self.err(E::Buf);
                    } else {
                        self.err(E::Misc);
                    }
                } else if up == "EQU" {
                    if self.statefine > 0 {
                        return SNIL;
                    } else if wdecl <= SLBL {
                        return self.equtbl(rest);
                    } else {
                        self.err(E::App);
                    }
                } else if opname_index(&up) < EQUOP {
                    let op = opname_index(&up);
                    if self.statefine > 0 {
                        return SNIL;
                    } else if wdecl <= SLBL {
                        let mut s = up.clone();
                        while idxp < b.len() && !is_space(b[idxp]) {
                            s.push(b[idxp] as char);
                            idxp += 1;
                        }
                        if is_space(at(b, idxp)) {
                            s.push(' ');
                        }
                        if !concat(dest, &s) {
                            self.err(E::Buf);
                        } else {
                            let w = self.trav2(
                                &buffer[idxp..],
                                dest,
                                if op < OPNUM { SVAL } else { SPSE },
                            );
                            if w != SERR {
                                if !self.symtbl.is_empty() {
                                    let names = std::mem::take(&mut self.symtbl);
                                    self.refs.push(Ref {
                                        names,
                                        kind: RefKind::Label(self.line),
                                        visit: false,
                                    });
                                }
                                if op < OPNUM {
                                    self.line += 1;
                                } else if op == ENDOP {
                                    self.vcont = false;
                                }
                                return SCOM;
                            }
                            return SERR;
                        }
                    } else {
                        self.err(E::App);
                    }
                } else if self.statefine == 0 {
                    let mut token = token;
                    let mut ok = true;
                    // name&counter: append a FOR counter as two digits.
                    loop {
                        if !(at(b, idxp) == b'&' && at(b, idxp + 1).is_ascii_alphabetic()) {
                            break;
                        }
                        idxp += 1;
                        let (t, name) = get_token(b, &mut idxp);
                        if t != Tt::Char || !ok {
                            break;
                        }
                        match self.lookup(&name).map(|k| self.refs[k].kind.clone()) {
                            Some(RefKind::Stack(v)) => {
                                let s = format!("{:02}", v);
                                if !concat(&mut token, &s) {
                                    self.err(E::Buf);
                                }
                            }
                            _ => {
                                self.err(E::Cat);
                                ok = false;
                            }
                        }
                    }
                    let rest = &buffer[idxp..];
                    if let Some(k) = self.lookup(&token) {
                        match self.refs[k].kind.clone() {
                            RefKind::Text(_) => {
                                if self.refs[k].visit {
                                    self.err(E::Rec);
                                } else {
                                    return self.equsub(rest, dest, wdecl, k);
                                }
                            }
                            kind if wdecl > SLBL => {
                                let mut s = match kind {
                                    RefKind::Stack(v) => format!("{:02}", v),
                                    RefKind::Label(v) if wdecl == SPSE => format!("{}", v),
                                    RefKind::Label(v) => format!("{}", v as i64 - self.line as i64),
                                    RefKind::Text(_) => unreachable!(),
                                };
                                if is_space(at(b, idxp)) {
                                    s.push(' ');
                                }
                                if concat(dest, &s) {
                                    return self.trav2(rest, dest, wdecl);
                                }
                                self.err(E::Buf);
                            }
                            _ => self.err(E::Idn),
                        }
                    } else if wdecl <= SLBL {
                        if self.symtbl.len() < GRPMAX {
                            self.symtbl.push(token);
                        } else {
                            self.err(E::Grp);
                        }
                        let mut j = idxp;
                        if at(b, j) == b':' {
                            j += 1;
                        }
                        return self.trav2(&buffer[j..], dest, SLBL);
                    } else {
                        if is_space(at(b, idxp)) {
                            token.push(' ');
                        }
                        if concat(dest, &token) {
                            return self.trav2(rest, dest, wdecl);
                        }
                        self.err(E::Buf);
                    }
                } else {
                    return self.trav2(rest, dest, SNIL);
                }
                SERR
            }
            _ => {
                if self.statefine > 0 {
                    return SNIL;
                } else if wdecl <= SLBL {
                    self.err(E::Tok);
                } else if concat(dest, &token) {
                    return self.trav2(&buffer[idxp..], dest, wdecl);
                } else {
                    self.err(E::Buf);
                }
                SERR
            }
        }
    }

    // ---- pass 2 --------------------------------------------------------------

    /// The CHARTOKEN branch shared by the operand-start states.
    fn char_ref(&mut self, expr: &str, idx: usize, tok: &str, state: St, single: St) {
        let rest = &expr[idx..];
        if let Some(k) = self.lookup(tok) {
            if self.refs[k].visit {
                self.err(E::Rec);
            } else if let RefKind::Text(list) = self.refs[k].kind {
                let text = self.lists[list]
                    .first()
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                self.refs[k].visit = true;
                self.automaton(&text, state);
                self.refs[k].visit = false;
                let ls = self.laststate;
                self.automaton(rest, ls);
            } else {
                let v = match self.refs[k].kind {
                    RefKind::Label(v) | RefKind::Stack(v) => v,
                    RefKind::Text(_) => unreachable!(),
                };
                let tmp = format!("{}", v as i64 - self.line as i64);
                self.refs[k].visit = true;
                self.automaton(&tmp, state);
                self.refs[k].visit = false;
                let ls = self.laststate;
                self.automaton(rest, ls);
            }
        } else if tok.len() == 1 {
            self.automaton(expr, single);
        } else {
            self.err(E::Snf);
        }
    }

    /// The CHARTOKEN branch of the expression-collecting states.
    fn char_expr(&mut self, expr: &str, idx: usize, tok: &str, b_side: bool) {
        let rest = &expr[idx..];
        let state = if b_side { St::Expr } else { St::ExpFs };
        if let Some(k) = self.lookup(tok) {
            if self.refs[k].visit {
                self.err(E::Rec);
            } else if let RefKind::Text(list) = self.refs[k].kind {
                let text = self.lists[list]
                    .first()
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                self.refs[k].visit = true;
                self.automaton(&text, state);
                self.refs[k].visit = false;
                let next = if b_side { St::Expr } else { self.laststate };
                self.automaton(rest, next);
            } else {
                let v = match self.refs[k].kind {
                    RefKind::Label(v) | RefKind::Stack(v) => v,
                    RefKind::Text(_) => unreachable!(),
                };
                let tmp = if !b_side || self.opcode < OPNUM {
                    format!("{}", v as i64 - self.line as i64)
                } else {
                    format!("{}", v)
                };
                self.refs[k].visit = true;
                self.automaton(&tmp, state);
                self.refs[k].visit = false;
                let next = if b_side { St::Expr } else { self.laststate };
                self.automaton(rest, next);
            }
        } else if tok.len() == 1 {
            let dest = if b_side {
                &mut self.b_expr
            } else {
                &mut self.a_expr
            };
            if concat(dest, tok) {
                self.filled = true;
                self.automaton(rest, state);
            } else {
                self.err(E::Buf);
            }
        } else {
            self.err(E::Snf);
        }
    }

    fn automaton(&mut self, expr: &str, state: St) {
        self.laststate = state;
        let b = expr.as_bytes();
        let mut idx = 0usize;
        match state {
            St::Op => {
                self.filled = false;
                let (tt, tok) = get_token(b, &mut idx);
                if tt == Tt::Char {
                    self.opcode = opname_index(&tok.to_ascii_uppercase());
                    if self.opcode < OPNUM {
                        self.automaton(&expr[idx..], St::ModAddrExp);
                    } else if self.opcode < EQUOP {
                        self.automaton(&expr[idx..], St::Expr);
                    } else {
                        self.err(E::Exp);
                    }
                } else {
                    self.err(E::Exp);
                }
            }
            St::ModAddrExp | St::AddrExpA | St::AddrExpB => {
                self.filled = false;
                let (tt, tok) = get_token(b, &mut idx);
                let (next, single) = if state == St::AddrExpB {
                    (St::Expr, St::Expr)
                } else {
                    (St::ExpFs, St::ExpFs)
                };
                match tt {
                    Tt::Modf if state == St::ModAddrExp => self.automaton(&expr[idx..], St::Modf),
                    Tt::Addr => {
                        let m = mode_of(tok.as_bytes()[0]);
                        if state == St::AddrExpB {
                            self.b_mode = m;
                        } else {
                            self.a_mode = m;
                        }
                        self.automaton(&expr[idx..], next);
                    }
                    Tt::Numb | Tt::Expr => self.automaton(expr, next),
                    Tt::Char => self.char_ref(expr, idx, &tok, state, single),
                    Tt::None => {}
                    _ if state == St::ModAddrExp => self.err(E::App),
                    _ => self.err(E::Exp),
                }
            }
            St::Modf => {
                self.filled = false;
                let (tt, tok) = get_token(b, &mut idx);
                match tt {
                    Tt::Char => match MODNAME.iter().position(|m| *m == tok.to_ascii_uppercase()) {
                        Some(m) => {
                            self.modifier = Some(m);
                            self.automaton(&expr[idx..], St::AddrExpA);
                        }
                        None => self.err(E::Exp),
                    },
                    Tt::None => {}
                    _ => self.err(E::Exp),
                }
            }
            St::ExpFs | St::Expr => {
                let b_side = state == St::Expr;
                let (tt, mut tok) = get_token(b, &mut idx);
                match tt {
                    Tt::Fsep if !b_side => self.automaton(&expr[idx..], St::AddrExpB),
                    Tt::Addr => {
                        if !matches!(tok.as_bytes()[0], b'>' | b'<' | b'*') {
                            self.err(E::App);
                        } else {
                            let dest = if b_side {
                                &mut self.b_expr
                            } else {
                                &mut self.a_expr
                            };
                            if !concat(dest, &tok) {
                                self.err(E::Buf);
                            }
                            self.filled = true;
                            self.automaton(&expr[idx..], state);
                        }
                    }
                    Tt::Numb | Tt::Expr => {
                        if tt == Tt::Numb && !concat(&mut tok, " ") {
                            self.err(E::Buf);
                        }
                        let dest = if b_side {
                            &mut self.b_expr
                        } else {
                            &mut self.a_expr
                        };
                        if !concat(dest, &tok) {
                            self.err(E::Buf);
                        }
                        self.filled = true;
                        self.automaton(&expr[idx..], state);
                    }
                    Tt::Char => self.char_expr(expr, idx, &tok, b_side),
                    Tt::None => {}
                    _ => self.err(E::App),
                }
            }
        }
    }

    /// dfashell(): parse one rewritten line into opcode, modifier, modes and
    /// the two field expressions.
    fn dfashell(&mut self, text: &str) {
        self.a_mode = Mode::Direct;
        self.b_mode = Mode::Direct;
        self.modifier = None;
        self.a_expr.clear();
        self.b_expr.clear();
        self.line_err = false;
        self.automaton(text, St::Op);
        if self.opcode < OPNUM {
            if !self.filled && !self.line_err {
                self.err(E::Syn);
            } else if self.b_expr.is_empty() {
                match OPNAME[self.opcode] {
                    "DAT" => {
                        self.b_mode = self.a_mode;
                        self.b_expr = std::mem::take(&mut self.a_expr);
                        self.a_mode = Mode::Immediate;
                        self.a_expr = "0".into();
                    }
                    "SPL" | "JMP" | "NOP" => {
                        self.b_mode = Mode::Direct;
                        self.b_expr = "0".into();
                    }
                    _ => self.err(E::Nop),
                }
            }
        }
    }

    fn encode(&mut self) -> Vec<Instruction> {
        let mut code = Vec::new();
        let count = self.line;
        if count > MAXINSTR {
            self.err_at(E::Exx, 0);
            return code;
        }
        if count as usize > self.cfg.max_length {
            self.err_at(E::Lin, 0);
        }
        if count == 0 {
            self.err_at(E::Zln, 0);
            return code;
        }
        self.line = 0;
        let out = std::mem::take(&mut self.out);
        self.lists.push(out.clone());
        let list = self.lists.len() - 1;
        let cs = self.cfg.core_size as i64;
        for (k, l) in out.iter().enumerate() {
            self.aline = Some((list, k));
            self.dfashell(&l.text);
            if self.errnum != 0 {
                continue;
            }
            if self.a_expr.is_empty() {
                self.a_expr = "0".into();
            }
            if self.b_expr.is_empty() {
                self.b_expr = "0".into();
            }
            if matches!(self.opcode, ORGOP | ENDOP | PINOP) {
                let e = self.b_expr.clone();
                let (st, v) = self.eval(&e);
                if st < OK_EXPR {
                    self.err(if st == DIV_ZERO { E::Div } else { E::Evl });
                } else {
                    if st == OVERFLOW {
                        self.err(E::Ofl);
                    }
                    match self.opcode {
                        ORGOP => self.offset = v.rem_euclid(cs),
                        PINOP => self.pin = Some(v),
                        _ => {
                            if v != 0 {
                                if self.offset != 0 {
                                    self.err(E::Doe);
                                } else {
                                    self.offset = v.rem_euclid(cs);
                                }
                            }
                        }
                    }
                }
                continue;
            }
            let (ea, eb) = (self.a_expr.clone(), self.b_expr.clone());
            let (sa, va) = self.eval(&ea);
            if sa < OK_EXPR {
                self.err(if sa == DIV_ZERO { E::Div } else { E::Evl });
                continue;
            }
            let (sb, vb) = self.eval(&eb);
            if sb < OK_EXPR {
                self.err(if sb == DIV_ZERO { E::Div } else { E::Evl });
                continue;
            }
            if sa == OVERFLOW || sb == OVERFLOW {
                self.err(E::Ofl);
            }
            let op = opcode_of(self.opcode);
            let modifier = match self.modifier {
                Some(m) => Modifier::ALL[m],
                None => crate::redcode::default_modifier(op, self.a_mode, self.b_mode),
            };
            code.push(Instruction {
                op,
                modifier,
                a_mode: self.a_mode,
                a: va.rem_euclid(cs) as u32,
                b_mode: self.b_mode,
                b: vb.rem_euclid(cs) as u32,
            });
            self.line += 1;
        }
        if self.offset < 0 || self.offset >= count as i64 {
            self.err_at(E::Ofs, 0);
        }
        code
    }

    fn run(&mut self, src: &str) -> Warrior {
        self.addpredefs();
        if self.cfg.first_warrior {
            self.regs[(b'W' - b'A') as usize] = self.cfg.warriors as i64;
            self.regs[(b'S' - b'A') as usize] = self.cfg.warriors as i64;
        }
        self.read(src);
        self.line = 0;
        self.expand();
        if !self.symtbl.is_empty() {
            self.symtbl.clear();
            self.err_at(E::Dlb, 0);
        }
        if self.noassert {
            self.err_at(E::Nas, 0);
        }
        let code = self.encode();
        Warrior {
            name: self.name.clone(),
            author: self.author.clone(),
            code,
            start: self.offset as u32,
            pin: self.pin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> (i32, i64) {
        let mut r = [0i64; 26];
        let mut so = 0u8;
        eval_expr(s, &mut r, &mut so)
    }

    #[test]
    fn evaluator_basics() {
        assert_eq!(ev("1+2*3"), (OK_EXPR, 7));
        assert_eq!(ev("(1+2)*3"), (OK_EXPR, 9));
        assert_eq!(ev("3--2"), (OK_EXPR, 5));
        assert_eq!(ev("7/0").0, DIV_ZERO);
        assert_eq!(ev("2<3&&3>2"), (OK_EXPR, 1));
        assert_eq!(ev("a=5"), (OK_EXPR, 5));
    }

    #[test]
    fn warrior_basics() {
        let w = assemble(
            ";redcode\n;assert 1\nstart mov 0, 1\n end start\n",
            &Config::default(),
        )
        .unwrap();
        assert_eq!(w.code.len(), 1);
        assert_eq!(w.code[0].op, Opcode::Mov);
        assert_eq!(w.code[0].modifier, Modifier::I);
    }
}

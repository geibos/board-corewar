//! Redcode assembler for the ICWS'94 subset the hill accepts.
//!
//! Accepted: labels (optionally followed by `:`), `EQU` (textual, like
//! pMARS: `x EQU 2+1` then `3*x` is `3*2+1`), `ORG`, `END [start]`,
//! expressions with C precedence (`|| && == != < > <= >= + - * / %`,
//! unary `- + !`, parentheses), the predefined constants pMARS knows, all
//! addressing modes and modifiers, `;name` and `;author`.
//!
//! Refused with a message: P-space (`LDP`, `STP`, `PIN`) and `FOR`/`ROF`
//! macros. They are real ICWS'94 extensions; the first hill runs without
//! them.

use crate::redcode::{default_modifier, Instruction, Mode, Modifier, Opcode, Warrior};
use std::collections::HashMap;
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "line {}: {}", self.line, self.msg)
        }
    }
}

impl std::error::Error for AsmError {}

fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, AsmError> {
    Err(AsmError {
        line,
        msg: msg.into(),
    })
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(i64),
    Sym(&'static str),
}

const SYMS2: [&str; 6] = ["==", "!=", "<=", ">=", "&&", "||"];
const SYMS1: [&str; 18] = [
    "+", "-", "*", "/", "%", "(", ")", ",", "#", "$", "@", "<", ">", "{", "}", "!", ":", ".",
];

fn tokenize(s: &str, line: usize) -> Result<Vec<Tok>, AsmError> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let st = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(s[st..i].to_string()));
        } else if c.is_ascii_digit() {
            let st = i;
            while i < b.len() && (b[i] as char).is_ascii_digit() {
                i += 1;
            }
            match s[st..i].parse::<i64>() {
                Ok(n) => out.push(Tok::Num(n)),
                Err(_) => return err(line, format!("number too large: {}", &s[st..i])),
            }
        } else if i + 1 < b.len() && SYMS2.contains(&&s[i..i + 2]) {
            let sym = SYMS2.iter().find(|x| **x == &s[i..i + 2]).unwrap();
            out.push(Tok::Sym(sym));
            i += 2;
        } else if let Some(sym) = SYMS1.iter().find(|x| x.as_bytes()[0] == b[i]) {
            out.push(Tok::Sym(sym));
            i += 1;
        } else {
            return err(line, format!("unexpected character '{}'", c));
        }
    }
    Ok(out)
}

enum Stmt {
    Instr {
        line: usize,
        op: Opcode,
        modifier: Option<Modifier>,
        operands: Vec<Vec<Tok>>,
    },
}

struct Ctx<'a> {
    cfg: &'a Config,
    labels: &'a HashMap<String, usize>,
    equs: &'a HashMap<String, Vec<Tok>>,
}

impl Ctx<'_> {
    fn constant(&self, name: &str, cur: usize) -> Option<i64> {
        let c = self.cfg;
        Some(match name {
            "CORESIZE" => c.core_size as i64,
            "MAXPROCESSES" => c.max_processes as i64,
            "MAXCYCLES" => c.max_cycles as i64,
            "MAXLENGTH" => c.max_length as i64,
            "MINDISTANCE" => c.min_distance as i64,
            "WARRIORS" => c.warriors as i64,
            "ROUNDS" => c.rounds as i64,
            "PSPACESIZE" => (c.core_size / 16) as i64,
            "VERSION" => 92,
            "CURLINE" => cur as i64,
            _ => return None,
        })
    }

    /// Replace EQU names by their token text, recursively, like pMARS does.
    fn expand(&self, toks: &[Tok], line: usize, depth: usize) -> Result<Vec<Tok>, AsmError> {
        if depth > 32 {
            return err(line, "EQU nested too deeply (a cycle?)");
        }
        let mut out = Vec::with_capacity(toks.len());
        for t in toks {
            match t {
                Tok::Ident(n) if self.equs.contains_key(n) => {
                    out.extend(self.expand(&self.equs[n], line, depth + 1)?);
                }
                _ => out.push(t.clone()),
            }
        }
        Ok(out)
    }

    fn eval(&self, toks: &[Tok], cur: usize, line: usize) -> Result<i64, AsmError> {
        let toks = self.expand(toks, line, 0)?;
        if toks.is_empty() {
            return err(line, "missing expression");
        }
        let mut p = Parser {
            t: &toks,
            i: 0,
            ctx: self,
            cur,
            line,
        };
        let v = p.or()?;
        if p.i != toks.len() {
            return err(line, format!("unexpected {:?} in expression", toks[p.i]));
        }
        Ok(v)
    }
}

struct Parser<'a, 'b> {
    t: &'a [Tok],
    i: usize,
    ctx: &'a Ctx<'b>,
    cur: usize,
    line: usize,
}

impl Parser<'_, '_> {
    fn peek(&self, s: &str) -> bool {
        matches!(self.t.get(self.i), Some(Tok::Sym(x)) if *x == s)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.peek(s) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn or(&mut self) -> Result<i64, AsmError> {
        let mut v = self.and()?;
        while self.eat("||") {
            let r = self.and()?;
            v = ((v != 0) || (r != 0)) as i64;
        }
        Ok(v)
    }
    fn and(&mut self) -> Result<i64, AsmError> {
        let mut v = self.cmp()?;
        while self.eat("&&") {
            let r = self.cmp()?;
            v = ((v != 0) && (r != 0)) as i64;
        }
        Ok(v)
    }
    fn cmp(&mut self) -> Result<i64, AsmError> {
        let mut v = self.add()?;
        loop {
            let op = ["==", "!=", "<=", ">=", "<", ">"]
                .iter()
                .find(|o| self.peek(o))
                .copied();
            let Some(op) = op else { return Ok(v) };
            self.i += 1;
            let r = self.add()?;
            v = match op {
                "==" => v == r,
                "!=" => v != r,
                "<=" => v <= r,
                ">=" => v >= r,
                "<" => v < r,
                _ => v > r,
            } as i64;
        }
    }
    fn add(&mut self) -> Result<i64, AsmError> {
        let mut v = self.mul()?;
        loop {
            if self.eat("+") {
                v = v.wrapping_add(self.mul()?);
            } else if self.eat("-") {
                v = v.wrapping_sub(self.mul()?);
            } else {
                return Ok(v);
            }
        }
    }
    fn mul(&mut self) -> Result<i64, AsmError> {
        let mut v = self.unary()?;
        loop {
            let op = ["*", "/", "%"].iter().find(|o| self.peek(o)).copied();
            let Some(op) = op else { return Ok(v) };
            self.i += 1;
            let r = self.unary()?;
            v = match op {
                "*" => v.wrapping_mul(r),
                _ if r == 0 => return err(self.line, "division by zero in expression"),
                "/" => v / r,
                _ => v % r,
            };
        }
    }
    fn unary(&mut self) -> Result<i64, AsmError> {
        if self.eat("-") {
            return Ok(self.unary()?.wrapping_neg());
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("!") {
            return Ok((self.unary()? == 0) as i64);
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<i64, AsmError> {
        match self.t.get(self.i).cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                if let Some(&at) = self.ctx.labels.get(&name) {
                    return Ok(at as i64 - self.cur as i64);
                }
                if let Some(v) = self.ctx.constant(&name, self.cur) {
                    return Ok(v);
                }
                err(self.line, format!("unknown label '{}'", name))
            }
            Some(Tok::Sym("(")) => {
                self.i += 1;
                let v = self.or()?;
                if !self.eat(")") {
                    return err(self.line, "missing ')'");
                }
                Ok(v)
            }
            Some(t) => err(self.line, format!("unexpected {:?} in expression", t)),
            None => err(self.line, "expression ends too early"),
        }
    }
}

fn split_operands(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut out = vec![Vec::new()];
    let mut depth = 0i32;
    for t in toks {
        match t {
            Tok::Sym("(") => depth += 1,
            Tok::Sym(")") => depth -= 1,
            Tok::Sym(",") if depth == 0 => {
                out.push(Vec::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(t.clone());
    }
    if out.len() == 1 && out[0].is_empty() {
        out.clear();
    }
    out
}

fn norm(v: i64, cs: u32) -> u32 {
    v.rem_euclid(cs as i64) as u32
}

pub fn assemble(src: &str, cfg: &Config) -> Result<Warrior, AsmError> {
    let mut name = String::from("Anonymous");
    let mut author = String::from("Anonymous");
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut equs: HashMap<String, Vec<Tok>> = HashMap::new();
    let mut stmts: Vec<Stmt> = Vec::new();
    let mut pending: Vec<(String, usize)> = Vec::new();
    let mut org: Option<(Vec<Tok>, usize)> = None;

    for (n, raw) in src.lines().enumerate() {
        let line = n + 1;
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix(";name") {
            name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(";author") {
            author = rest.trim().to_string();
            continue;
        }
        let code = raw.split(';').next().unwrap_or("");
        let toks = tokenize(code, line)?;
        if toks.is_empty() {
            continue;
        }
        // Labels: identifiers before the opcode or pseudo-op, each optionally
        // followed by ':'.
        let mut i = 0;
        let mut here: Vec<String> = Vec::new();
        let mut word: Option<String> = None;
        while let Some(Tok::Ident(id)) = toks.get(i) {
            let up = id.to_ascii_uppercase();
            if Opcode::parse(&up).is_some()
                || matches!(
                    up.as_str(),
                    "EQU" | "ORG" | "END" | "LDP" | "STP" | "PIN" | "FOR" | "ROF"
                )
            {
                word = Some(up);
                i += 1;
                break;
            }
            here.push(id.clone());
            i += 1;
            if matches!(toks.get(i), Some(Tok::Sym(":"))) {
                i += 1;
            }
        }
        let Some(word) = word else {
            if i < toks.len() {
                return err(line, format!("expected an opcode, found {:?}", toks[i]));
            }
            // A line with labels only: they name the next instruction.
            pending.extend(here.into_iter().map(|l| (l, line)));
            continue;
        };
        match word.as_str() {
            "EQU" => {
                let mut names: Vec<String> = pending.drain(..).map(|(l, _)| l).collect();
                names.extend(here);
                if names.is_empty() {
                    return err(line, "EQU needs a name");
                }
                for nm in names {
                    if labels.contains_key(&nm) || equs.contains_key(&nm) {
                        return err(line, format!("'{}' is defined twice", nm));
                    }
                    equs.insert(nm, toks[i..].to_vec());
                }
                continue;
            }
            "LDP" | "STP" | "PIN" => {
                return err(
                    line,
                    format!("{} needs P-space, which this hill does not support", word),
                )
            }
            "FOR" | "ROF" => {
                return err(
                    line,
                    "FOR/ROF macros are not supported; write the lines out",
                )
            }
            _ => {}
        }
        // Any other statement takes the labels in front of it.
        let at = stmts.len();
        for l in pending.drain(..).map(|(l, _)| l).chain(here) {
            if labels.contains_key(&l) || equs.contains_key(&l) {
                return err(line, format!("'{}' is defined twice", l));
            }
            labels.insert(l, at);
        }
        match word.as_str() {
            "ORG" => {
                if i >= toks.len() {
                    return err(line, "ORG needs a start address");
                }
                org = Some((toks[i..].to_vec(), line));
                continue;
            }
            "END" => {
                if i < toks.len() {
                    org = Some((toks[i..].to_vec(), line));
                }
                break;
            }
            _ => {}
        }
        let op = Opcode::parse(&word).unwrap();
        let mut modifier = None;
        if matches!(toks.get(i), Some(Tok::Sym("."))) {
            match toks.get(i + 1) {
                Some(Tok::Ident(m)) => match Modifier::parse(m) {
                    Some(m) => modifier = Some(m),
                    None => return err(line, format!("unknown modifier '{}'", m)),
                },
                _ => return err(line, "modifier expected after '.'"),
            }
            i += 2;
        }
        let operands = split_operands(&toks[i..]);
        if operands.is_empty() {
            return err(line, format!("{} needs at least one operand", op.name()));
        }
        if operands.len() > 2 || operands.iter().any(|o| o.is_empty()) {
            return err(line, "an instruction takes one or two operands");
        }
        stmts.push(Stmt::Instr {
            line,
            op,
            modifier,
            operands,
        });
    }

    if stmts.is_empty() {
        return err(0, "no instructions");
    }
    if stmts.len() > cfg.max_length {
        return err(
            0,
            format!(
                "{} instructions, the limit is {}",
                stmts.len(),
                cfg.max_length
            ),
        );
    }
    for (l, line) in &pending {
        // Labels after the last instruction point one past the end.
        labels.entry(l.clone()).or_insert(stmts.len());
        let _ = line;
    }

    let ctx = Ctx {
        cfg,
        labels: &labels,
        equs: &equs,
    };
    let cs = cfg.core_size;
    let mut code = Vec::with_capacity(stmts.len());
    for (
        idx,
        Stmt::Instr {
            line,
            op,
            modifier,
            operands,
        },
    ) in stmts.iter().enumerate()
    {
        let operand = |toks: &Vec<Tok>| -> Result<(Mode, u32), AsmError> {
            let (mode, rest) = match toks.first() {
                Some(Tok::Sym(s))
                    if s.len() == 1 && Mode::from_char(s.chars().next().unwrap()).is_some() =>
                {
                    (
                        Mode::from_char(s.chars().next().unwrap()).unwrap(),
                        &toks[1..],
                    )
                }
                _ => (Mode::Direct, &toks[..]),
            };
            Ok((mode, norm(ctx.eval(rest, idx, *line)?, cs)))
        };
        let ((a_mode, a), (b_mode, b)) = if operands.len() == 1 {
            let only = operand(&operands[0])?;
            if *op == Opcode::Dat {
                ((Mode::Immediate, 0), only)
            } else {
                (only, (Mode::Direct, 0))
            }
        } else {
            (operand(&operands[0])?, operand(&operands[1])?)
        };
        let modifier = modifier.unwrap_or_else(|| default_modifier(*op, a_mode, b_mode));
        code.push(Instruction {
            op: *op,
            modifier,
            a_mode,
            a,
            b_mode,
            b,
        });
    }

    let start = match org {
        Some((toks, line)) => ctx.eval(&toks, 0, line)?,
        None => 0,
    };
    if start < 0 || start as usize >= code.len() {
        return err(
            0,
            format!("start {} is outside the program (0..{})", start, code.len()),
        );
    }
    Ok(Warrior {
        name,
        author,
        code,
        start: start as u32,
    })
}

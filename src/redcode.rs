//! Redcode as ICWS'94 defines it: opcodes, modifiers, addressing modes and
//! the instruction. Field values are kept normalized to `0..core_size`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opcode {
    Dat,
    Mov,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Jmp,
    Jmz,
    Jmn,
    Djn,
    Spl,
    Slt,
    Seq,
    Sne,
    Nop,
}

impl Opcode {
    pub fn parse(s: &str) -> Option<Opcode> {
        Some(match s.to_ascii_uppercase().as_str() {
            "DAT" => Opcode::Dat,
            "MOV" => Opcode::Mov,
            "ADD" => Opcode::Add,
            "SUB" => Opcode::Sub,
            "MUL" => Opcode::Mul,
            "DIV" => Opcode::Div,
            "MOD" => Opcode::Mod,
            "JMP" => Opcode::Jmp,
            "JMZ" => Opcode::Jmz,
            "JMN" => Opcode::Jmn,
            "DJN" => Opcode::Djn,
            "SPL" => Opcode::Spl,
            "SLT" => Opcode::Slt,
            // CMP is the '88 name of SEQ; they are one opcode.
            "CMP" | "SEQ" => Opcode::Seq,
            "SNE" => Opcode::Sne,
            "NOP" => Opcode::Nop,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Opcode::Dat => "DAT",
            Opcode::Mov => "MOV",
            Opcode::Add => "ADD",
            Opcode::Sub => "SUB",
            Opcode::Mul => "MUL",
            Opcode::Div => "DIV",
            Opcode::Mod => "MOD",
            Opcode::Jmp => "JMP",
            Opcode::Jmz => "JMZ",
            Opcode::Jmn => "JMN",
            Opcode::Djn => "DJN",
            Opcode::Spl => "SPL",
            Opcode::Slt => "SLT",
            Opcode::Seq => "SEQ",
            Opcode::Sne => "SNE",
            Opcode::Nop => "NOP",
        }
    }

    pub const ALL: [Opcode; 16] = [
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
        Opcode::Seq,
        Opcode::Sne,
        Opcode::Nop,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    A,
    B,
    AB,
    BA,
    F,
    X,
    I,
}

impl Modifier {
    pub fn parse(s: &str) -> Option<Modifier> {
        Some(match s.to_ascii_uppercase().as_str() {
            "A" => Modifier::A,
            "B" => Modifier::B,
            "AB" => Modifier::AB,
            "BA" => Modifier::BA,
            "F" => Modifier::F,
            "X" => Modifier::X,
            "I" => Modifier::I,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Modifier::A => "A",
            Modifier::B => "B",
            Modifier::AB => "AB",
            Modifier::BA => "BA",
            Modifier::F => "F",
            Modifier::X => "X",
            Modifier::I => "I",
        }
    }

    pub const ALL: [Modifier; 7] = [
        Modifier::A,
        Modifier::B,
        Modifier::AB,
        Modifier::BA,
        Modifier::F,
        Modifier::X,
        Modifier::I,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// `#`
    Immediate,
    /// `$`
    Direct,
    /// `*`
    AIndirect,
    /// `@`
    BIndirect,
    /// `{`
    APredec,
    /// `<`
    BPredec,
    /// `}`
    APostinc,
    /// `>`
    BPostinc,
}

impl Mode {
    pub fn from_char(c: char) -> Option<Mode> {
        Some(match c {
            '#' => Mode::Immediate,
            '$' => Mode::Direct,
            '*' => Mode::AIndirect,
            '@' => Mode::BIndirect,
            '{' => Mode::APredec,
            '<' => Mode::BPredec,
            '}' => Mode::APostinc,
            '>' => Mode::BPostinc,
            _ => return None,
        })
    }

    pub fn symbol(self) -> char {
        match self {
            Mode::Immediate => '#',
            Mode::Direct => '$',
            Mode::AIndirect => '*',
            Mode::BIndirect => '@',
            Mode::APredec => '{',
            Mode::BPredec => '<',
            Mode::APostinc => '}',
            Mode::BPostinc => '>',
        }
    }

    pub const ALL: [Mode; 8] = [
        Mode::Immediate,
        Mode::Direct,
        Mode::AIndirect,
        Mode::BIndirect,
        Mode::APredec,
        Mode::BPredec,
        Mode::APostinc,
        Mode::BPostinc,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Instruction {
    pub op: Opcode,
    pub modifier: Modifier,
    pub a_mode: Mode,
    pub a: u32,
    pub b_mode: Mode,
    pub b: u32,
}

impl Instruction {
    /// The empty core cell: `DAT.F $0, $0`.
    pub const EMPTY: Instruction = Instruction {
        op: Opcode::Dat,
        modifier: Modifier::F,
        a_mode: Mode::Direct,
        a: 0,
        b_mode: Mode::Direct,
        b: 0,
    };
}

/// ICWS'94 default modifier for an instruction written without one.
pub fn default_modifier(op: Opcode, a_mode: Mode, b_mode: Mode) -> Modifier {
    use Opcode::*;
    match op {
        Dat | Nop => Modifier::F,
        Mov | Seq | Sne => {
            if a_mode == Mode::Immediate {
                Modifier::AB
            } else if b_mode == Mode::Immediate {
                Modifier::B
            } else {
                Modifier::I
            }
        }
        Add | Sub | Mul | Div | Mod => {
            if a_mode == Mode::Immediate {
                Modifier::AB
            } else if b_mode == Mode::Immediate {
                Modifier::B
            } else {
                Modifier::F
            }
        }
        Slt => {
            if a_mode == Mode::Immediate {
                Modifier::AB
            } else {
                Modifier::B
            }
        }
        Jmp | Jmz | Jmn | Djn | Spl => Modifier::B,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warrior {
    pub name: String,
    pub author: String,
    pub code: Vec<Instruction>,
    /// Offset of the first instruction to execute, relative to the load address.
    pub start: u32,
}

/// Signed rendering of a normalized field, the way pMARS prints listings:
/// values above half the core are shown as negative offsets.
pub fn signed(v: u32, core_size: u32) -> i64 {
    let v = v as i64;
    if v > core_size as i64 / 2 {
        v - core_size as i64
    } else {
        v
    }
}

pub struct Listing<'a> {
    pub warrior: &'a Warrior,
    pub core_size: u32,
}

impl fmt::Display for Listing<'_> {
    /// One instruction per line: `MOV.I $0, $1`. Used to compare the
    /// assembler with pMARS's listing.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ORG {}", self.warrior.start)?;
        for i in &self.warrior.code {
            writeln!(
                f,
                "{}.{} {}{}, {}{}",
                i.op.name(),
                i.modifier.name(),
                i.a_mode.symbol(),
                signed(i.a, self.core_size),
                i.b_mode.symbol(),
                signed(i.b, self.core_size)
            )?;
        }
        Ok(())
    }
}

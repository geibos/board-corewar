//! What `cw --json` prints: one document per command, the same facts as the
//! text output. Warriors are referred to by their index in `warriors`.
use crate::asm::Config;
use crate::mars::Score;
use crate::redcode::Warrior;
use serde::Serialize;

/// The match parameters, named after pMARS's flags' meaning.
#[derive(Serialize)]
pub struct Params {
    pub core_size: u32,
    pub cycles: u32,
    pub processes: u32,
    pub length: usize,
    pub distance: u32,
    pub rounds: u32,
}

impl From<&Config> for Params {
    fn from(c: &Config) -> Params {
        Params {
            core_size: c.core_size,
            cycles: c.max_cycles,
            processes: c.max_processes,
            length: c.max_length,
            distance: c.min_distance,
            rounds: c.rounds,
        }
    }
}

#[derive(Serialize)]
pub struct WarriorInfo {
    pub file: String,
    pub name: String,
    pub author: String,
    /// Instructions.
    pub length: usize,
}

impl WarriorInfo {
    pub fn new(file: &str, w: &Warrior) -> WarriorInfo {
        WarriorInfo {
            file: file.to_owned(),
            name: w.name.clone(),
            author: w.author.clone(),
            length: w.code.len(),
        }
    }
}

/// Work done and time taken: the one part that differs between runs.
#[derive(Serialize)]
pub struct Stats {
    pub instructions: u64,
    pub seconds: f64,
}

/// `cw check`: one entry per file; name, author and length when it
/// assembles, `error` when it does not.
#[derive(Serialize)]
pub struct Checked {
    pub file: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Checked {
    pub fn new(file: &str, r: Result<Warrior, String>) -> Checked {
        let (w, error) = match r {
            Ok(w) => (Some(w), None),
            Err(e) => (None, Some(e)),
        };
        Checked {
            file: file.to_owned(),
            ok: w.is_some(),
            name: w.as_ref().map(|w| w.name.clone()),
            author: w.as_ref().map(|w| w.author.clone()),
            length: w.as_ref().map(|w| w.code.len()),
            error,
        }
    }
}

/// `cw list`: the program as pMARS lists it, one instruction a line, and
/// the offset execution starts at.
#[derive(Serialize)]
pub struct Listed {
    pub warrior: WarriorInfo,
    pub start: u32,
    pub code: Vec<String>,
}

#[derive(Serialize)]
pub struct Pair {
    pub params: Params,
    pub warriors: [WarriorInfo; 2],
    /// Position seed: pMARS's -F minus the distance.
    pub seed: u32,
    pub result: Score,
    pub stats: Stats,
}

#[derive(Serialize)]
pub struct Battle {
    pub params: Params,
    pub warriors: [WarriorInfo; 2],
    pub pos: u32,
    pub first: u8,
    pub result: Score,
}

/// One match of a round robin: `a` was pMARS's first warrior.
#[derive(Serialize)]
pub struct Match {
    pub a: usize,
    pub b: usize,
    pub seed: u32,
    pub result: Score,
}

#[derive(Serialize)]
pub struct Standing {
    pub place: usize,
    pub warrior: usize,
    /// 3 a win, 1 a tie.
    pub score: u64,
}

#[derive(Serialize)]
pub struct Tournament {
    pub params: Params,
    pub warriors: Vec<WarriorInfo>,
    pub matches: Vec<Match>,
    pub standings: Vec<Standing>,
    pub stats: Stats,
}

/// `cw trace`: the match, a summary of every round and the recorded
/// rounds' frames (see `crate::trace`). Always JSON.
#[derive(Serialize)]
pub struct Traced {
    pub params: Params,
    pub warriors: [WarriorInfo; 2],
    pub seed: u32,
    pub score: Score,
    pub rounds: Vec<crate::trace::RoundSummary>,
    pub recorded: Vec<crate::trace::Recording>,
}

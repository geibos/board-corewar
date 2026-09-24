//! A hill: warriors kept in a directory, ranked by their matches against
//! each other; a challenger plays every member, and past `size` the lowest
//! fall off.
//!
//! ```text
//! DIR/hill.toml      the rules, edited by hand
//! DIR/state.json     the members, best first, with age and arrival
//! DIR/results.json   every match between members, and the fingerprint of
//!                    the rules they were played under
//! DIR/warriors/      sources, named by the SHA-256 of their text
//! DIR/history.jsonl  one line per challenge
//! ```
//!
//! A match depends only on its two warriors and the rules: the one whose id
//! sorts first moves first (pMARS's first warrior), and the position seed is
//! derived from both ids. So a result is the same whenever and in whatever
//! order warriors arrive, and it is cached until the rules that decide it
//! (parameters and rounds) change. Points, size and the tie rule only rank.
use crate::asm::{assemble, read_source, Config};
use crate::fast::Compiled;
use crate::mars::Score;
use crate::multi::Job;
use crate::pool;
use crate::report::Stats;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

/// Which of two warriors with equal scores ranks higher.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TieBreak {
    /// The one on the hill longer: a challenger must beat, not match.
    Older,
    Newer,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Members kept; 0 keeps everyone (a ladder).
    pub size: usize,
    /// Rounds per match.
    pub rounds: u32,
    pub tie_break: TieBreak,
    pub params: MatchParams,
    pub points: Points,
}

/// pMARS's -s -c -p -l -d.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MatchParams {
    pub core_size: u32,
    pub cycles: u32,
    pub processes: u32,
    pub length: u32,
    pub distance: u32,
}

/// Per round won, tied and lost.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Points {
    pub win: i64,
    pub tie: i64,
    pub loss: i64,
}

const RULES_HEADER: &str = "\
# The rules of this hill (cw hill). Edit freely: matches the change does
# not affect are kept, the rest are replayed by the next `cw hill challenge`.
#
# size       members kept; 0 keeps everyone (a ladder)
# rounds     rounds per match
# tie_break  on equal scores, \"older\" or \"newer\" ranks higher
# [params]   pMARS's -s -c -p -l -d
# [points]   per round won, tied, lost

";

impl Rules {
    /// The '94 rules for a match configuration: 3 points a win, 1 a tie,
    /// and on equal scores the older warrior stays.
    pub fn new(cfg: &Config, size: usize) -> Rules {
        Rules {
            size,
            rounds: cfg.rounds,
            tie_break: TieBreak::Older,
            params: MatchParams {
                core_size: cfg.core_size,
                cycles: cfg.max_cycles,
                processes: cfg.max_processes,
                length: cfg.max_length as u32,
                distance: cfg.min_distance,
            },
            points: Points {
                win: 3,
                tie: 1,
                loss: 0,
            },
        }
    }

    pub fn config(&self) -> Config {
        Config {
            core_size: self.params.core_size,
            max_cycles: self.params.cycles,
            max_processes: self.params.processes,
            max_length: self.params.length as usize,
            min_distance: self.params.distance,
            rounds: self.rounds,
            ..Config::default()
        }
    }

    /// pMARS's checks on the parameters, and at least one round.
    pub fn check(&self) -> Result<()> {
        if self.rounds == 0 {
            return Err("rounds: at least 1".into());
        }
        self.config().check(2)
    }

    /// What decides a match: the parameters and the rounds.
    fn fingerprint(&self) -> String {
        let p = &self.params;
        hash(
            format!(
                "cw-hill 1 s{} c{} p{} l{} d{} r{}",
                p.core_size, p.cycles, p.processes, p.length, p.distance, self.rounds
            )
            .as_bytes(),
        )
    }
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// A warrior's id: the SHA-256 of its source, first 16 hex digits.
pub fn warrior_id(source: &str) -> String {
    hash(source.as_bytes())[..16].to_owned()
}

/// The two ids of a match in playing order, and its cache key.
fn pairing<'a>(x: &'a str, y: &'a str) -> (&'a str, &'a str, String) {
    let (a, b) = if x < y { (x, y) } else { (y, x) };
    (a, b, format!("{}:{}", a, b))
}

/// The position seed of a match, from both ids: pMARS's -F is
/// seed + distance.
fn seed(a: &str, b: &str, positions: u32) -> u32 {
    let d = Sha256::digest(format!("{}:{}", a, b).as_bytes());
    let mut n = [0u8; 8];
    n.copy_from_slice(&d[..8]);
    (u64::from_be_bytes(n) % positions as u64) as u32
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub author: String,
    /// The file name it was submitted as.
    pub file: String,
    /// Challenge number it arrived with; smaller is older.
    pub arrived: u64,
    /// Challenges survived since.
    pub age: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    /// The next arrival number.
    next: u64,
    /// Best first.
    members: Vec<Member>,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    fingerprint: String,
    /// "a:b" (a moved first) to the score from a's side.
    matches: BTreeMap<String, Score>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// On the hill after its challenge.
    Entered,
    /// Played, and ranked below the hill's size.
    PushedOff,
    /// Unreadable, or refused by the assembler under the hill's rules.
    Rejected,
    /// The same source is on the hill already, or earlier in the list.
    Duplicate,
}

#[derive(Serialize)]
pub struct Challenger {
    pub file: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its place right after its challenge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Gone {
    pub id: String,
    pub name: String,
    pub author: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct Played {
    /// Moved first.
    pub a: String,
    pub b: String,
    pub seed: u32,
    pub result: Score,
}

#[derive(Serialize, Clone, Debug)]
pub struct Standing {
    pub place: usize,
    pub id: String,
    pub name: String,
    pub author: String,
    pub score: i64,
    pub wins: u32,
    pub ties: u32,
    pub losses: u32,
    pub age: u32,
}

#[derive(Serialize)]
pub struct ChallengeReport {
    pub rules: Rules,
    pub challengers: Vec<Challenger>,
    pub pushed_off: Vec<Gone>,
    /// Matches played this time; the rest came from results.json.
    pub played: Vec<Played>,
    pub standings: Vec<Standing>,
    pub stats: Stats,
}

#[derive(Serialize)]
pub struct ShowReport {
    pub rules: Rules,
    pub standings: Vec<Standing>,
}

/// An open hill. Holds an exclusive lock on DIR/.lock until dropped, so
/// that two runs on one hill wait for each other.
pub struct Hill {
    dir: PathBuf,
    pub rules: Rules,
    state: State,
    cache: Cache,
    _lock: File,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e))
}

/// Replace a file whole: a crash leaves the old one or the new one.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {}", path.display(), e))
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).expect("hill state serializes") + "\n"
}

impl Hill {
    /// Make DIR a hill with these rules; refuses a directory that is one.
    pub fn init(dir: &Path, rules: &Rules) -> Result<()> {
        rules.check()?;
        let path = dir.join("hill.toml");
        if path.exists() {
            return Err(format!("{}: a hill already", dir.display()));
        }
        std::fs::create_dir_all(dir.join("warriors"))
            .map_err(|e| format!("{}: {}", dir.display(), e))?;
        let toml = toml::to_string(rules).map_err(|e| e.to_string())?;
        write_atomic(&dir.join("state.json"), &to_json(&State::default()))?;
        write_atomic(
            &dir.join("results.json"),
            &to_json(&Cache {
                fingerprint: rules.fingerprint(),
                matches: BTreeMap::new(),
            }),
        )?;
        // Last: its presence is what makes DIR a hill.
        write_atomic(&path, &format!("{}{}", RULES_HEADER, toml))
    }

    pub fn open(dir: &Path) -> Result<Hill> {
        let rules_path = dir.join("hill.toml");
        let text = std::fs::read_to_string(&rules_path).map_err(|e| {
            format!(
                "{}: {} (make one with `cw hill init`)",
                rules_path.display(),
                e
            )
        })?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".lock"))
            .map_err(|e| format!("{}: {}", dir.display(), e))?;
        lock.lock()
            .map_err(|e| format!("{}/.lock: {}", dir.display(), e))?;
        let rules: Rules =
            toml::from_str(&text).map_err(|e| format!("{}: {}", rules_path.display(), e))?;
        rules
            .check()
            .map_err(|e| format!("{}: {}", rules_path.display(), e))?;
        let state: State = read_json(&dir.join("state.json"))?;
        let mut cache: Cache = read_json(&dir.join("results.json"))?;
        if cache.fingerprint != rules.fingerprint() {
            cache = Cache {
                fingerprint: rules.fingerprint(),
                matches: BTreeMap::new(),
            };
        }
        Ok(Hill {
            dir: dir.to_owned(),
            rules,
            state,
            cache,
            _lock: lock,
        })
    }

    fn source_path(&self, id: &str) -> PathBuf {
        self.dir.join("warriors").join(format!("{}.red", id))
    }

    /// Play `files` against the hill, one challenger after another in the
    /// order given, on `threads` threads; with no files, replay what a
    /// change of rules made missing and rank again. Saves the hill.
    pub fn challenge(&mut self, files: &[String], threads: usize) -> Result<ChallengeReport> {
        let t0 = std::time::Instant::now();
        let cfg = self.rules.config();
        let second = Config {
            first_warrior: false,
            ..cfg
        };
        let mut challengers = Vec::new();
        let mut entrants: Vec<(usize, Member)> = Vec::new();
        for f in files {
            let file = Path::new(f)
                .file_name()
                .map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned());
            let checked = read_source(Path::new(f))
                .map_err(|e| e.to_string())
                .and_then(|src| {
                    let w = assemble(&src, &cfg).map_err(|e| e.to_string())?;
                    assemble(&src, &second).map_err(|e| e.to_string())?;
                    Ok((src, w))
                });
            let (src, w) = match checked {
                Ok(x) => x,
                Err(e) => {
                    challengers.push(Challenger {
                        file,
                        status: Status::Rejected,
                        id: None,
                        name: None,
                        place: None,
                        error: Some(e),
                    });
                    continue;
                }
            };
            let id = warrior_id(&src);
            let known = self.state.members.iter().any(|m| m.id == id)
                || entrants.iter().any(|(_, m)| m.id == id);
            if !known {
                write_atomic(&self.source_path(&id), &src)?;
            }
            challengers.push(Challenger {
                file: file.clone(),
                status: if known {
                    Status::Duplicate
                } else {
                    Status::Entered
                },
                id: Some(id.clone()),
                name: Some(w.name.clone()),
                place: None,
                error: None,
            });
            if !known {
                entrants.push((
                    challengers.len() - 1,
                    Member {
                        id,
                        name: w.name,
                        author: w.author,
                        file,
                        arrived: 0,
                        age: 0,
                    },
                ));
            }
        }

        // Members that no longer assemble under the rules leave.
        let mut pushed_off = Vec::new();
        let mut compiled: HashMap<String, (Compiled, Compiled)> = HashMap::new();
        let mut kept = Vec::new();
        let everyone = std::mem::take(&mut self.state.members)
            .into_iter()
            .map(|m| (None, m))
            .chain(entrants.into_iter().map(|(i, m)| (Some(i), m)));
        for (slot, m) in everyone {
            let built = read_source(&self.source_path(&m.id))
                .map_err(|e| e.to_string())
                .and_then(|src| {
                    let w1 = assemble(&src, &cfg).map_err(|e| e.to_string())?;
                    let w2 = assemble(&src, &second).map_err(|e| e.to_string())?;
                    Ok((Compiled::new(&w1), Compiled::new(&w2)))
                });
            match built {
                Ok(c) => {
                    compiled.insert(m.id.clone(), c);
                    kept.push((slot, m));
                }
                Err(e) => pushed_off.push(Gone {
                    id: m.id,
                    name: m.name,
                    author: m.author,
                    reason: format!("does not assemble under the rules: {}", e),
                }),
            }
        }
        let (incumbents, entrants): (Vec<_>, Vec<_>) =
            kept.into_iter().partition(|(s, _)| s.is_none());
        self.state.members = incumbents.into_iter().map(|(_, m)| m).collect();

        // Every missing match among members and challengers, at once.
        let ids: Vec<&str> = self
            .state
            .members
            .iter()
            .chain(entrants.iter().map(|(_, m)| m))
            .map(|m| m.id.as_str())
            .collect();
        let positions = cfg.core_size + 1 - 2 * cfg.min_distance;
        let mut missing = Vec::new();
        for (i, x) in ids.iter().enumerate() {
            for y in &ids[i + 1..] {
                let (a, b, key) = pairing(x, y);
                if !self.cache.matches.contains_key(&key) {
                    missing.push((a, b, key, seed(a, b, positions)));
                }
            }
        }
        let jobs: Vec<Job> = missing
            .iter()
            .map(|(a, b, _, s)| Job {
                a: &compiled[*a].0,
                b: &compiled[*b].1,
                seed: *s as i32,
            })
            .collect();
        let (scores, instructions) = pool::play_all(&cfg, &jobs, cfg.rounds, threads);
        let mut played = Vec::new();
        for ((a, b, key, s), result) in missing.into_iter().zip(scores) {
            self.cache.matches.insert(key, result);
            played.push(Played {
                a: a.to_owned(),
                b: b.to_owned(),
                seed: s,
                result,
            });
        }

        // Challengers join one at a time.
        for (slot, mut m) in entrants {
            m.arrived = self.state.next;
            self.state.next += 1;
            let id = m.id.clone();
            self.state.members.push(m);
            self.rank()?;
            let place = self.state.members.iter().position(|m| m.id == id);
            for gone in self.trim() {
                pushed_off.push(gone);
            }
            for m in &mut self.state.members {
                if m.id != id {
                    m.age += 1;
                }
            }
            if let Some(slot) = slot {
                let c = &mut challengers[slot];
                c.place = place.map(|p| p + 1);
                if !self.state.members.iter().any(|m| m.id == id) {
                    c.status = Status::PushedOff;
                }
            }
        }
        // Rules may have changed the ranking or the size.
        self.rank()?;
        pushed_off.extend(self.trim());

        let members: std::collections::HashSet<&str> =
            self.state.members.iter().map(|m| m.id.as_str()).collect();
        self.cache.matches.retain(|k, _| {
            k.split_once(':')
                .is_some_and(|(a, b)| members.contains(a) && members.contains(b))
        });
        self.save(&challengers, &pushed_off)?;
        Ok(ChallengeReport {
            rules: self.rules.clone(),
            challengers,
            pushed_off,
            played,
            standings: self.standings()?,
            stats: Stats {
                instructions,
                seconds: t0.elapsed().as_secs_f64(),
            },
        })
    }

    /// The table as it stands; no match is played.
    pub fn show(&self) -> Result<ShowReport> {
        Ok(ShowReport {
            rules: self.rules.clone(),
            standings: self.standings()?,
        })
    }

    /// Each member's score and record against the others, in member order.
    fn records(&self) -> Result<Vec<Standing>> {
        let p = self.rules.points;
        let ms = &self.state.members;
        let mut out: Vec<Standing> = ms
            .iter()
            .enumerate()
            .map(|(i, m)| Standing {
                place: i + 1,
                id: m.id.clone(),
                name: m.name.clone(),
                author: m.author.clone(),
                score: 0,
                wins: 0,
                ties: 0,
                losses: 0,
                age: m.age,
            })
            .collect();
        for i in 0..ms.len() {
            for j in i + 1..ms.len() {
                let (a, _, key) = pairing(&ms[i].id, &ms[j].id);
                let s = self.cache.matches.get(&key).ok_or_else(|| {
                    format!(
                        "no result for {} vs {}: the rules changed; `cw hill challenge {}` replays it",
                        ms[i].name,
                        ms[j].name,
                        self.dir.display()
                    )
                })?;
                // From i's side.
                let (w, l) = if a == ms[i].id {
                    (s.w1, s.w2)
                } else {
                    (s.w2, s.w1)
                };
                for (k, (won, lost)) in [(i, (w, l)), (j, (l, w))] {
                    let r = &mut out[k];
                    r.wins += won;
                    r.losses += lost;
                    r.ties += s.ties;
                    r.score += p.win * won as i64 + p.tie * s.ties as i64 + p.loss * lost as i64;
                }
            }
        }
        Ok(out)
    }

    /// Order the members: score, then the tie rule.
    fn rank(&mut self) -> Result<()> {
        let recs = self.records()?;
        let score: HashMap<&str, i64> = recs.iter().map(|r| (r.id.as_str(), r.score)).collect();
        let mut ms = std::mem::take(&mut self.state.members);
        let newer = self.rules.tie_break == TieBreak::Newer;
        ms.sort_by(|x, y| {
            score[y.id.as_str()]
                .cmp(&score[x.id.as_str()])
                .then_with(|| {
                    if newer {
                        y.arrived.cmp(&x.arrived)
                    } else {
                        x.arrived.cmp(&y.arrived)
                    }
                })
        });
        self.state.members = ms;
        Ok(())
    }

    /// Drop members past the size.
    fn trim(&mut self) -> Vec<Gone> {
        if self.rules.size == 0 || self.state.members.len() <= self.rules.size {
            return Vec::new();
        }
        self.state
            .members
            .split_off(self.rules.size)
            .into_iter()
            .map(|m| Gone {
                id: m.id,
                name: m.name,
                author: m.author,
                reason: "pushed off".into(),
            })
            .collect()
    }

    fn standings(&self) -> Result<Vec<Standing>> {
        self.records()
    }

    fn save(&self, challengers: &[Challenger], pushed_off: &[Gone]) -> Result<()> {
        write_atomic(&self.dir.join("results.json"), &to_json(&self.cache))?;
        write_atomic(&self.dir.join("state.json"), &to_json(&self.state))?;
        if challengers.is_empty() && pushed_off.is_empty() {
            return Ok(());
        }
        #[derive(Serialize)]
        struct Line<'a> {
            time: u64,
            challengers: &'a [Challenger],
            pushed_off: &'a [Gone],
        }
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let line = serde_json::to_string(&Line {
            time,
            challengers,
            pushed_off,
        })
        .expect("history line serializes");
        use std::io::Write;
        let path = self.dir.join("history.jsonl");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| writeln!(f, "{}", line))
            .map_err(|e| format!("{}: {}", path.display(), e))
    }
}

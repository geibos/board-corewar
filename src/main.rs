//! `cw` — the hill's command line. `cw help` and `cw help COMMAND` list the
//! commands and their flags. Match parameters take pMARS's flags and
//! defaults (-s -c -p -l -d), checked as pMARS checks them.

use clap::{Args, Parser, Subcommand};
use corewar::asm::{assemble, Config};
use corewar::fast::{Compiled, Engine};
use corewar::hill::{self, Hill};
use corewar::mars::{Outcome, Score};
use corewar::multi::{Job, Multi};
use corewar::pool;
use corewar::redcode::{Listing, Warrior};
use corewar::report::{self, Params as ParamsReport, Stats, WarriorInfo};
use corewar::trace;
use sha2::{Digest, Sha256};
use std::process::exit;

#[derive(Parser)]
#[command(name = "cw", version, about = "Core War like pMARS 0.9.2, faster")]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Print one JSON document instead of text.
    #[arg(long, global = true)]
    json: bool,
}

fn print_json<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).expect("serialize"));
}

#[derive(Subcommand)]
enum Command {
    /// Assemble; print name, author and length, or the error.
    Check {
        #[arg(required = true)]
        files: Vec<String>,
        #[command(flatten)]
        params: Params,
    },
    /// The assembled program, one instruction per line.
    List {
        file: String,
        #[command(flatten)]
        params: Params,
    },
    /// A match, like `pmars -b -r ROUNDS -F SEED+DISTANCE A B`.
    Pair {
        a: String,
        b: String,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(1..))]
        rounds: u32,
        /// Position seed: pMARS's -F minus the distance.
        #[arg(long, default_value_t = 1)]
        seed: u32,
        #[command(flatten)]
        params: Params,
    },
    /// A match on the plain engine with the core's writes recorded, as one
    /// JSON document (always, --json or not): a summary of every round and,
    /// for the rounds in --record, a frame every --every cycles of which
    /// warrior wrote which cells. Placed like `cw pair`; warrior 0 is A.
    Trace {
        a: String,
        b: String,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(1..))]
        rounds: u32,
        /// Position seed, as in `cw pair`.
        #[arg(long, default_value_t = 1)]
        seed: u32,
        /// Rounds to record, from 1: `--record 1,17,117`.
        #[arg(long, value_delimiter = ',')]
        record: Vec<u32>,
        /// Cycles between frames.
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..))]
        every: u32,
        #[command(flatten)]
        params: Params,
    },
    /// One battle, like `pmars -b -r 1 -F POS A B`.
    Battle {
        a: String,
        b: String,
        /// Where B is loaded; default half the core.
        #[arg(long)]
        pos: Option<u32>,
        /// Which warrior moves first.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=1))]
        first: u8,
        #[command(flatten)]
        params: Params,
    },
    /// Every pair plays a match; pair k is placed like `pmars -F X` with
    /// X = DISTANCE + 997k mod (CORE + 1 - 2 DISTANCE). Prints each pair's
    /// result and a score table (win 3, tie 1: pMARS's default formula).
    Tournament {
        #[arg(required = true, num_args = 2..)]
        files: Vec<String>,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(1..))]
        rounds: u32,
        /// Threads to play matches on; 0: one per CPU. Same results.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        /// 2 interleaves two matches on one thread (src/multi.rs).
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
        lanes: u8,
        #[command(flatten)]
        params: Params,
    },
    /// Each warrior plays each opponent on each seed, all in one run, and
    /// gets its points (3 a win, 1 a tie; with --hill, the hill's) over the
    /// seeds. Matches are placed like `cw pair`, the warrior as A. A
    /// directory stands for its *.red files. A warrior does not play its own
    /// copy; files that do not assemble are named and left out.
    Versus {
        #[arg(required = true)]
        files: Vec<String>,
        /// Opponents: files and directories.
        #[arg(long, num_args = 1.., required_unless_present = "hill")]
        against: Vec<String>,
        /// A hill: its members are opponents, best first, and its rules
        /// give the parameters, the rounds and the points.
        #[arg(long, conflicts_with_all = ["rounds", "core_size", "cycles", "processes", "length", "distance"])]
        hill: Option<String>,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(1..))]
        rounds: u32,
        /// Position seeds, as in `cw pair`: `--seed 1,17,3900`.
        #[arg(long, value_delimiter = ',', default_values_t = [1], conflicts_with = "seeds")]
        seed: Vec<u32>,
        /// N seeds from --salt: seed k (from 0) is the first 8 bytes of
        /// sha256("cw versus SALT k") as a big-endian number, mod
        /// CORE + 1 - 2 DISTANCE.
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        seeds: Option<u32>,
        /// For --seeds; default 0.
        #[arg(long, requires = "seeds")]
        salt: Option<u64>,
        /// Threads to play matches on; 0: one per CPU. Same results.
        #[arg(long, default_value_t = 0)]
        jobs: usize,
        /// Also each warrior's result against each opponent (text; --json
        /// always has every match).
        #[arg(long)]
        per: bool,
        #[command(flatten)]
        params: Params,
    },
    /// A hill kept in a directory: its rules (hill.toml), members and every
    /// match between them. `cw hill init DIR`, then `cw hill challenge DIR
    /// FILE...`.
    Hill {
        #[command(subcommand)]
        command: HillCommand,
    },
}

#[derive(Subcommand)]
enum HillCommand {
    /// Make DIR a hill. The rules go to DIR/hill.toml, which can be edited
    /// later: points, tie rule and everything below.
    Init {
        dir: String,
        /// Members kept; 0 keeps everyone (a ladder).
        #[arg(long, default_value_t = 20)]
        size: usize,
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u32).range(1..))]
        rounds: u32,
        /// Where a match places the second warrior: "hash", from the two
        /// warriors' ids, or "random", from a number drawn at each challenge.
        #[arg(long, default_value = "hash", value_parser = ["hash", "random"])]
        placement: String,
        #[command(flatten)]
        params: Params,
    },
    /// Each file challenges the hill in turn: it plays every member, and
    /// past the size the lowest fall off. Matches already played are not
    /// played again. With no files, replays what a change of rules left
    /// missing.
    Challenge {
        dir: String,
        files: Vec<String>,
        /// Threads to play matches on; 0: one per CPU. Same results.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        /// On a hill with placement = "random": play from this number
        /// instead of drawing one (to replay a challenge from its output).
        #[arg(long)]
        seed: Option<u64>,
    },
    /// The table, best first.
    Show { dir: String },
    /// Replay the members on fresh placements, without changing the hill:
    /// run k of a pair A:B (A the smaller id, moving first) is placed by the
    /// first 8 bytes of sha256("SEED:A:B:K"), big-endian, mod CORE + 1 -
    /// 2 DISTANCE. The table comes from these matches alone. For a final
    /// table after a freeze, from a value published after it.
    Recount {
        dir: String,
        /// The text the placements come from.
        #[arg(long)]
        seed: String,
        /// Matches per pair.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        runs: u32,
        /// Threads to play matches on; 0: one per CPU. Same results.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
    },
    /// Check the hill without changing it: sources against their ids, every
    /// stored match played again, the table against the rules. Exit code 1
    /// when anything differs.
    Verify {
        dir: String,
        /// Threads to replay matches on; 0: one per CPU.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
    },
}

/// pMARS's match parameters, its flags and bounds. The core is limited to
/// 65535 cells here (16-bit addresses); pMARS allows up to 2^30.
#[derive(Args)]
struct Params {
    /// Size of core.
    #[arg(short = 's', default_value_t = 8000, value_parser = clap::value_parser!(u32).range(1..=65535))]
    core_size: u32,
    /// Cycles until tie.
    #[arg(short = 'c', default_value_t = 80000, value_parser = clap::value_parser!(u32).range(1..))]
    cycles: u32,
    /// Max. processes.
    #[arg(short = 'p', default_value_t = 8000, value_parser = clap::value_parser!(u32).range(1..=i32::MAX as i64))]
    processes: u32,
    /// Max. warrior length.
    #[arg(short = 'l', default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=1000))]
    length: u32,
    /// Min. warriors distance; default the length, as in pMARS.
    #[arg(short = 'd', value_parser = clap::value_parser!(u32).range(1..=65535))]
    distance: Option<u32>,
}

impl Params {
    /// The configuration for a match of `rounds` rounds, or exit 2 with
    /// pMARS's reason. ROUNDS is visible to warriors, so a match assembles
    /// with its own count.
    fn config(&self, rounds: u32) -> Config {
        let cfg = Config {
            core_size: self.core_size,
            max_cycles: self.cycles,
            max_processes: self.processes,
            max_length: self.length as usize,
            min_distance: self.distance.unwrap_or(self.length),
            rounds,
            ..Config::default()
        };
        if let Err(e) = cfg.check(2) {
            eprintln!("error: {}", e);
            exit(2);
        }
        cfg
    }
}

/// pMARS gives registers W and S values only for the first warrior it
/// assembles: the configuration for the others.
fn second(cfg: &Config) -> Config {
    Config {
        first_warrior: false,
        ..*cfg
    }
}

fn load(path: &str, cfg: &Config) -> Warrior {
    let src = corewar::asm::read_source(path.as_ref()).unwrap_or_else(|e| {
        eprintln!("{}: {}", path, e);
        exit(2)
    });
    assemble(&src, cfg).unwrap_or_else(|e| {
        eprintln!("{}: {}", path, e);
        exit(1)
    })
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    match cli.command {
        Command::Check { files, params } => {
            let cfg = params.config(1);
            let checked: Vec<report::Checked> = files
                .iter()
                .map(|p| {
                    let r = corewar::asm::read_source(p.as_ref())
                        .map_err(|e| e.to_string())
                        .and_then(|s| assemble(&s, &cfg).map_err(|e| e.to_string()));
                    report::Checked::new(p, r)
                })
                .collect();
            if json {
                print_json(&checked);
            } else {
                for c in &checked {
                    match &c.error {
                        None => println!(
                            "{}: ok, \"{}\" by {}, {} instructions",
                            c.file,
                            c.name.as_deref().unwrap_or_default(),
                            c.author.as_deref().unwrap_or_default(),
                            c.length.unwrap_or_default()
                        ),
                        Some(e) => println!("{}: {}", c.file, e),
                    }
                }
            }
            exit(checked.iter().any(|c| !c.ok) as i32);
        }
        Command::List { file, params } => {
            let cfg = params.config(1);
            let w = load(&file, &cfg);
            let listing = Listing {
                warrior: &w,
                core_size: cfg.core_size,
            }
            .to_string();
            if json {
                print_json(&report::Listed {
                    warrior: WarriorInfo::new(&file, &w),
                    start: w.start,
                    // The first line is ORG, given as `start`.
                    code: listing.lines().skip(1).map(str::to_owned).collect(),
                });
            } else {
                print!("{}", listing);
            }
        }
        Command::Pair {
            a,
            b,
            rounds,
            seed,
            params,
        } => {
            let cfg = params.config(rounds);
            let (wa, wb) = (load(&a, &cfg), load(&b, &second(&cfg)));
            let (ca, cb) = (Compiled::new(&wa), Compiled::new(&wb));
            let mut mars = Engine::new(&cfg);
            let t0 = std::time::Instant::now();
            let s = mars.play(&cfg, &ca, &cb, rounds, seed as i32);
            let dt = t0.elapsed().as_secs_f64();
            if json {
                print_json(&report::Pair {
                    params: ParamsReport::from(&cfg),
                    warriors: [WarriorInfo::new(&a, &wa), WarriorInfo::new(&b, &wb)],
                    seed,
                    result: s,
                    stats: Stats {
                        instructions: mars.steps,
                        seconds: dt,
                    },
                });
                return;
            }
            println!("Results: {} {} {}", s.w1, s.w2, s.ties);
            eprintln!(
                "{} rounds, {} instructions, {:.3} s, {:.1} M instructions/s",
                rounds,
                mars.steps,
                dt,
                mars.steps as f64 / dt / 1e6
            );
        }
        Command::Trace {
            a,
            b,
            rounds,
            seed,
            record,
            every,
            params,
        } => {
            if let Some(r) = record.iter().find(|&&r| r == 0 || r > rounds) {
                eprintln!("--record: round {} is not in 1..={}", r, rounds);
                exit(2);
            }
            let cfg = params.config(rounds);
            let (wa, wb) = (load(&a, &cfg), load(&b, &second(&cfg)));
            let t = trace::trace(&cfg, &wa, &wb, rounds, seed as i32, &record, every);
            let doc = report::Traced {
                params: ParamsReport::from(&cfg),
                warriors: [WarriorInfo::new(&a, &wa), WarriorInfo::new(&b, &wb)],
                seed,
                score: t.score,
                rounds: t.rounds,
                recorded: t.recorded,
            };
            println!("{}", serde_json::to_string(&doc).expect("serialize"));
        }
        Command::Battle {
            a,
            b,
            pos,
            first,
            params,
        } => {
            let cfg = params.config(1);
            let pos = pos.unwrap_or(cfg.core_size / 2);
            if pos < cfg.min_distance {
                eprintln!(
                    "error: --pos {}: position of warrior #2 cannot be smaller than warrior distance (-d {})",
                    pos, cfg.min_distance
                );
                exit(2);
            }
            let (wa, wb) = (load(&a, &cfg), load(&b, &second(&cfg)));
            let (ca, cb) = (Compiled::new(&wa), Compiled::new(&wb));
            let mut mars = Engine::new(&cfg);
            let (w1, w2, ties) = match mars.battle(&cfg, [&ca, &cb], [0, pos], first as usize) {
                Outcome::Win(0) => (1, 0, 0),
                Outcome::Win(_) => (0, 1, 0),
                Outcome::Tie => (0, 0, 1),
            };
            if json {
                print_json(&report::Battle {
                    params: ParamsReport::from(&cfg),
                    warriors: [WarriorInfo::new(&a, &wa), WarriorInfo::new(&b, &wb)],
                    pos,
                    first,
                    result: Score { w1, w2, ties },
                });
                return;
            }
            println!("Results: {} {} {}", w1, w2, ties);
        }
        Command::Tournament {
            files,
            rounds,
            jobs,
            lanes,
            params,
        } => {
            let cfg = params.config(rounds);
            let threads = match jobs {
                0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
                n => n,
            };
            if lanes == 2 && threads > 1 {
                eprintln!("error: --lanes 2 plays on one thread; drop --jobs");
                exit(2);
            }
            tournament(&cfg, &files, threads, lanes == 2, json);
        }
        Command::Versus {
            files,
            against,
            hill,
            rounds,
            seed,
            seeds,
            salt,
            jobs,
            per,
            params,
        } => {
            let (cfg, points, mut opponents) = match hill {
                Some(dir) => {
                    let h = or_exit(Hill::open(std::path::Path::new(&dir)));
                    let files = h
                        .members()
                        .into_iter()
                        .map(|(_, p)| p.to_string_lossy().into_owned())
                        .collect();
                    (h.rules.config(), h.rules.points, files)
                }
                None => (
                    params.config(rounds),
                    hill::Points {
                        win: 3,
                        tie: 1,
                        loss: 0,
                    },
                    Vec::new(),
                ),
            };
            opponents.extend(expand(&against));
            let positions = cfg.core_size + 1 - 2 * cfg.min_distance;
            let seeds = match seeds {
                Some(n) => (0..n)
                    .map(|k| salted(salt.unwrap_or(0), k, positions))
                    .collect(),
                None => seed,
            };
            let run = Versus {
                cfg: &cfg,
                points,
                seeds: &seeds,
                threads: threads(jobs),
            };
            run.play(&expand(&files), &opponents, per, json);
        }
        Command::Hill { command } => hill_command(command, json),
    }
}

/// The files that FILE and DIR arguments stand for: a directory, its *.red
/// files sorted by name. Exit 2 on a path that cannot be read.
fn expand(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let fail = |e: std::io::Error| -> ! {
            eprintln!("{}: {}", p, e);
            exit(2)
        };
        let path = std::path::Path::new(p);
        if !std::fs::metadata(path).unwrap_or_else(|e| fail(e)).is_dir() {
            out.push(p.clone());
            continue;
        }
        let mut reds: Vec<String> = std::fs::read_dir(path)
            .and_then(|d| {
                d.map(|e| e.map(|e| e.path()))
                    .collect::<std::io::Result<Vec<_>>>()
            })
            .unwrap_or_else(|e| fail(e))
            .into_iter()
            .filter(|f| f.is_file() && f.extension().is_some_and(|x| x == "red"))
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        reds.sort();
        out.extend(reds);
    }
    out
}

/// Seed `k` of `--seeds N --salt SALT`: the first 8 bytes of
/// sha256("cw versus SALT k"), big-endian, mod `positions`.
fn salted(salt: u64, k: u32, positions: u32) -> u32 {
    let d = Sha256::digest(format!("cw versus {} {}", salt, k).as_bytes());
    let n = u64::from_be_bytes(d[..8].try_into().expect("8 bytes"));
    (n % positions as u64) as u32
}

/// The warriors of one side of `cw versus` that assembled.
struct Side {
    info: Vec<WarriorInfo>,
    /// As on a hill: the start of the source's SHA-256.
    ids: Vec<String>,
    code: Vec<Compiled>,
}

impl Side {
    fn load(files: &[String], cfg: &Config, rejected: &mut Vec<report::Rejected>) -> Side {
        let mut side = Side {
            info: Vec::new(),
            ids: Vec::new(),
            code: Vec::new(),
        };
        for f in files {
            let r = corewar::asm::read_source(f.as_ref())
                .map_err(|e| e.to_string())
                .and_then(|src| {
                    let w = assemble(&src, cfg).map_err(|e| e.to_string())?;
                    Ok((src, w))
                });
            match r {
                Ok((src, w)) => {
                    side.info.push(WarriorInfo::new(f, &w));
                    side.ids.push(hill::warrior_id(&src));
                    side.code.push(Compiled::new(&w));
                }
                Err(error) => rejected.push(report::Rejected {
                    file: f.clone(),
                    error,
                }),
            }
        }
        side
    }
}

/// Rounds won, tied and lost.
#[derive(Clone, Copy, Default)]
struct Record {
    wins: u64,
    ties: u64,
    losses: u64,
}

impl Record {
    fn add(&mut self, s: &Score) {
        self.wins += s.w1 as u64;
        self.ties += s.ties as u64;
        self.losses += s.w2 as u64;
    }

    fn points(&self, p: &hill::Points) -> i64 {
        self.wins as i64 * p.win + self.ties as i64 * p.tie + self.losses as i64 * p.loss
    }
}

struct Versus<'a> {
    cfg: &'a Config,
    points: hill::Points,
    seeds: &'a [u32],
    threads: usize,
}

impl Versus<'_> {
    fn play(&self, files: &[String], opponents: &[String], per: bool, json: bool) {
        let cfg = self.cfg;
        let mut rejected = Vec::new();
        let ws = Side::load(files, cfg, &mut rejected);
        let os = Side::load(opponents, &second(cfg), &mut rejected);
        for r in &rejected {
            eprintln!("{}: {} (not played)", r.file, r.error);
        }
        if ws.code.is_empty() {
            eprintln!("error: no warrior assembled");
            exit(2);
        }
        if os.code.is_empty() {
            eprintln!("error: no opponent assembled");
            exit(2);
        }
        let mut jobs = Vec::new();
        let mut keys = Vec::new();
        let mut copies = Vec::new();
        for i in 0..ws.code.len() {
            for j in 0..os.code.len() {
                if ws.ids[i] == os.ids[j] {
                    eprintln!(
                        "{} and {}: the same warrior, not played",
                        ws.info[i].file, os.info[j].file
                    );
                    copies.push([i, j]);
                    continue;
                }
                for &seed in self.seeds {
                    jobs.push(Job {
                        a: &ws.code[i],
                        b: &os.code[j],
                        seed: seed as i32,
                    });
                    keys.push((i, j, seed));
                }
            }
        }
        let t0 = std::time::Instant::now();
        let (results, steps) = pool::play_all(cfg, &jobs, cfg.rounds, self.threads);
        let dt = t0.elapsed().as_secs_f64();
        let mut table = vec![vec![None::<Record>; os.code.len()]; ws.code.len()];
        for (&(i, j, _), s) in keys.iter().zip(&results) {
            table[i][j].get_or_insert_with(Record::default).add(s);
        }
        let n = self.seeds.len() as f64;
        let totals: Vec<Record> = table
            .iter()
            .map(|row| {
                row.iter().flatten().fold(Record::default(), |mut t, r| {
                    t.wins += r.wins;
                    t.ties += r.ties;
                    t.losses += r.losses;
                    t
                })
            })
            .collect();
        // Stable: equal scores keep the order of the command line.
        let mut order: Vec<usize> = (0..ws.code.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(totals[i].points(&self.points)));
        let standings: Vec<report::VersusStanding> = order
            .iter()
            .enumerate()
            .map(|(place, &i)| report::VersusStanding {
                place: place + 1,
                warrior: i,
                score: totals[i].points(&self.points) as f64 / n,
                opponents: table[i].iter().flatten().count(),
                wins: totals[i].wins,
                ties: totals[i].ties,
                losses: totals[i].losses,
            })
            .collect();
        if json {
            print_json(&report::Versus {
                params: ParamsReport::from(cfg),
                points: self.points,
                warriors: ws.info,
                opponents: os.info,
                seeds: self.seeds.to_vec(),
                matches: keys
                    .iter()
                    .zip(&results)
                    .map(|(&(a, b, seed), &result)| report::Match { a, b, seed, result })
                    .collect(),
                standings,
                copies,
                rejected,
                stats: Stats {
                    instructions: steps,
                    seconds: dt,
                },
            });
            return;
        }
        println!(
            "{:>3} {:>10} {:>4} {:>7} {:>7} {:>7}  name (file)",
            "#", "score", "opp", "W", "T", "L"
        );
        for s in &standings {
            let w = &ws.info[s.warrior];
            println!(
                "{:3} {:10.1} {:4} {:7} {:7} {:7}  {} ({})",
                s.place, s.score, s.opponents, s.wins, s.ties, s.losses, w.name, w.file
            );
        }
        if per {
            for s in &standings {
                let w = &ws.info[s.warrior];
                println!();
                println!("{} ({}):", w.name, w.file);
                println!(
                    "   {:>10} {:>7} {:>7} {:>7}  opponent (file)",
                    "score", "W", "T", "L"
                );
                for (j, r) in table[s.warrior].iter().enumerate() {
                    let Some(r) = r else { continue };
                    let o = &os.info[j];
                    println!(
                        "   {:10.1} {:7} {:7} {:7}  {} ({})",
                        r.points(&self.points) as f64 / n,
                        r.wins,
                        r.ties,
                        r.losses,
                        o.name,
                        o.file
                    );
                }
            }
        }
        eprintln!(
            "{} matches x {} rounds, {} instructions, {:.3} s, {:.1} M instructions/s",
            jobs.len(),
            cfg.rounds,
            steps,
            dt,
            steps as f64 / dt / 1e6
        );
    }
}

fn threads(jobs: usize) -> usize {
    match jobs {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    }
}

fn or_exit<T>(r: hill::Result<T>) -> T {
    r.unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        exit(2)
    })
}

fn hill_command(command: HillCommand, json: bool) {
    match command {
        HillCommand::Init {
            dir,
            size,
            rounds,
            placement,
            params,
        } => {
            let mut rules = hill::Rules::new(&params.config(rounds), size);
            if placement == "random" {
                rules.placement = hill::Placement::Random;
            }
            or_exit(Hill::init(std::path::Path::new(&dir), &rules));
            if !json {
                println!(
                    "{}: a hill of {} (0: no limit); rules in hill.toml",
                    dir, size
                );
            }
        }
        HillCommand::Challenge {
            dir,
            files,
            jobs,
            seed,
        } => {
            let mut h = or_exit(Hill::open(std::path::Path::new(&dir)));
            let r = or_exit(h.challenge_seeded(&files, threads(jobs), seed));
            if json {
                print_json(&r);
                return;
            }
            if let Some(s) = r.seed {
                println!("placement drawn from {}", s);
            }
            for c in &r.challengers {
                let what = match c.status {
                    hill::Status::Entered => format!("entered at {}", c.place.unwrap_or(0)),
                    hill::Status::PushedOff => {
                        format!("pushed off (placed {})", c.place.unwrap_or(0))
                    }
                    hill::Status::Rejected => {
                        format!("rejected: {}", c.error.as_deref().unwrap_or(""))
                    }
                    hill::Status::Duplicate => "already on the hill".into(),
                };
                match &c.name {
                    Some(n) => println!("{} ({}): {}", c.file, n, what),
                    None => println!("{}: {}", c.file, what),
                }
            }
            for g in &r.pushed_off {
                println!("off: {} by {} ({})", g.name, g.author, g.reason);
            }
            print_standings(&r.standings);
            eprintln!(
                "{} matches played, {} instructions, {:.3} s",
                r.played.len(),
                r.stats.instructions,
                r.stats.seconds
            );
        }
        HillCommand::Verify { dir, jobs } => {
            let h = or_exit(Hill::open(std::path::Path::new(&dir)));
            let r = or_exit(h.verify(threads(jobs)));
            if json {
                print_json(&r);
            } else {
                for p in &r.problems {
                    println!("{}: {}", p.kind, p.detail);
                }
                println!(
                    "{}: {} members, {} matches replayed, {} problems ({:.1} s)",
                    if r.ok { "ok" } else { "NOT OK" },
                    r.members,
                    r.replayed,
                    r.problems.len(),
                    r.stats.seconds
                );
            }
            exit(!r.ok as i32);
        }
        HillCommand::Recount {
            dir,
            seed,
            runs,
            jobs,
        } => {
            let h = or_exit(Hill::open(std::path::Path::new(&dir)));
            let r = or_exit(h.recount(&seed, runs, threads(jobs)));
            if json {
                print_json(&r);
                return;
            }
            println!("recount: {} matches a pair, placed from {}", r.runs, r.seed);
            print_standings(&r.standings);
            eprintln!(
                "{} matches played, {} instructions, {:.3} s",
                r.matches.len(),
                r.stats.instructions,
                r.stats.seconds
            );
        }
        HillCommand::Show { dir } => {
            let h = or_exit(Hill::open(std::path::Path::new(&dir)));
            let r = or_exit(h.show());
            if json {
                print_json(&r);
            } else {
                print_standings(&r.standings);
            }
        }
    }
}

fn print_standings(rows: &[hill::Standing]) {
    println!("  #    score     W     T     L  age  name");
    for r in rows {
        println!(
            "{:3} {:8} {:5} {:5} {:5} {:4}  {} by {} [{}]",
            r.place, r.score, r.wins, r.ties, r.losses, r.age, r.name, r.author, r.id
        );
    }
}

fn tournament(cfg: &Config, files: &[String], threads: usize, lanes: bool, json: bool) {
    let named: Vec<(Warrior, Compiled, Compiled)> = files
        .iter()
        .map(|f| {
            let w = load(f, cfg);
            let c2 = Compiled::new(&load(f, &second(cfg)));
            let c1 = Compiled::new(&w);
            (w, c1, c2)
        })
        .collect();
    let ws: Vec<(&Compiled, &Compiled)> = named.iter().map(|(_, a, b)| (a, b)).collect();
    let mut score = vec![0u64; ws.len()];
    let mut jobs = Vec::new();
    let mut pairs = Vec::new();
    // pmars -F X seeds its position generator with X - distance.
    let positions = (cfg.core_size + 1 - 2 * cfg.min_distance) as u64;
    for i in 0..ws.len() {
        for j in i + 1..ws.len() {
            let k = jobs.len() as u64;
            jobs.push(Job {
                a: ws[i].0,
                b: ws[j].1,
                seed: (k * 997 % positions) as i32,
            });
            pairs.push((i, j));
        }
    }
    let t0 = std::time::Instant::now();
    let (results, steps): (Vec<Score>, u64) = if lanes {
        let mut m = Multi::new(cfg);
        let r = m.play_all(cfg, &jobs, cfg.rounds);
        (r, m.steps)
    } else {
        pool::play_all(cfg, &jobs, cfg.rounds, threads)
    };
    let dt = t0.elapsed().as_secs_f64();
    for (&(i, j), s) in pairs.iter().zip(&results) {
        score[i] += 3 * s.w1 as u64 + s.ties as u64;
        score[j] += 3 * s.w2 as u64 + s.ties as u64;
    }
    // Stable: equal scores keep the order of the command line.
    let mut order: Vec<usize> = (0..ws.len()).collect();
    order.sort_by(|&x, &y| score[y].cmp(&score[x]));
    if json {
        print_json(&report::Tournament {
            params: ParamsReport::from(cfg),
            warriors: files
                .iter()
                .zip(&named)
                .map(|(f, (w, _, _))| WarriorInfo::new(f, w))
                .collect(),
            matches: pairs
                .iter()
                .zip(&jobs)
                .zip(&results)
                .map(|((&(a, b), job), &result)| report::Match {
                    a,
                    b,
                    seed: job.seed as u32,
                    result,
                })
                .collect(),
            standings: order
                .iter()
                .enumerate()
                .map(|(place, &i)| report::Standing {
                    place: place + 1,
                    warrior: i,
                    score: score[i],
                })
                .collect(),
            stats: Stats {
                instructions: steps,
                seconds: dt,
            },
        });
        return;
    }
    for (&(i, j), s) in pairs.iter().zip(&results) {
        println!(
            "{} vs {}: Results: {} {} {}",
            files[i], files[j], s.w1, s.w2, s.ties
        );
    }
    for (place, &i) in order.iter().enumerate() {
        println!(
            "{:3}. {:8} {} ({})",
            place + 1,
            score[i],
            named[i].0.name,
            files[i]
        );
    }
    eprintln!(
        "{} pairs x {} rounds, {} instructions, {:.3} s, {:.1} M instructions/s",
        jobs.len(),
        cfg.rounds,
        steps,
        dt,
        steps as f64 / dt / 1e6
    );
}

//! `cw` — the hill's command line. `cw help` and `cw help COMMAND` list the
//! commands and their flags. Match parameters take pMARS's flags and
//! defaults (-s -c -p -l -d), checked as pMARS checks them.

use clap::{Args, Parser, Subcommand};
use corewar::asm::{assemble, Config};
use corewar::fast::{Compiled, Engine};
use corewar::mars::{Outcome, Score};
use corewar::multi::{Job, Multi};
use corewar::pool;
use corewar::redcode::{Listing, Warrior};
use corewar::report::{self, Params as ParamsReport, Stats, WarriorInfo};
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
                    let r = std::fs::read_to_string(p)
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

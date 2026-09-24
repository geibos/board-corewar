//! `cw` — the hill's command line.
//!
//!   cw --version
//!
//!   cw check FILE...                 assemble, print name/author/length or the error
//!   cw list FILE                     the assembled program, one instruction per line
//!   cw pair A B [--rounds N] [--seed S]
//!                                    a match of N rounds (default 250), like `pmars -b -r N`
//!   cw battle A B --pos N [--first 0|1]
//!                                    one battle, B loaded at N; prints `Results: W1 W2 T`
//!                                    exactly like `pmars -b -r 1 -F N A B`
//!   cw tournament FILE... [--rounds N] [--jobs N] [--lanes 1|2]
//!                                    every pair plays a match; pair k is placed like
//!                                    `pmars -r N -F X` with X = 100 + 997k mod 7801;
//!                                    prints each pair's result and a score table
//!                                    (win 3, tie 1, pMARS's default formula);
//!                                    --jobs N plays matches on N threads (0: one per
//!                                    CPU; default 1), same results (see src/pool.rs);
//!                                    --lanes 2 interleaves two matches (see src/multi.rs)

use corewar::asm::{assemble, Config};
use corewar::fast::{Compiled, Engine};
use corewar::mars::{Outcome, Score};
use corewar::multi::{Job, Multi};
use corewar::pool;
use corewar::redcode::{Listing, Warrior};
use std::process::exit;

fn load(path: &str, cfg: &Config) -> Warrior {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{}: {}", path, e);
        exit(2)
    });
    assemble(&src, cfg).unwrap_or_else(|e| {
        eprintln!("{}: {}", path, e);
        exit(1)
    })
}

fn flag(args: &[String], name: &str) -> Option<u32> {
    let i = args.iter().position(|a| a == name)?;
    Some(args.get(i + 1)?.parse().unwrap_or_else(|_| {
        eprintln!("{} needs a number", name);
        exit(2)
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // ROUNDS is visible to warriors, so a match assembles with its own count.
    let cfg = Config {
        rounds: if matches!(
            args.first().map(String::as_str),
            Some("pair" | "tournament")
        ) {
            flag(&args, "--rounds").unwrap_or(250)
        } else {
            1
        },
        ..Config::default()
    };
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("cw {}", env!("CARGO_PKG_VERSION")),
        Some("check") if args.len() > 1 => {
            let mut bad = false;
            for p in &args[1..] {
                match std::fs::read_to_string(p).map(|s| assemble(&s, &cfg)) {
                    Ok(Ok(w)) => println!(
                        "{}: ok, \"{}\" by {}, {} instructions",
                        p,
                        w.name,
                        w.author,
                        w.code.len()
                    ),
                    Ok(Err(e)) => {
                        println!("{}: {}", p, e);
                        bad = true;
                    }
                    Err(e) => {
                        println!("{}: {}", p, e);
                        bad = true;
                    }
                }
            }
            exit(bad as i32);
        }
        Some("list") if args.len() == 2 => {
            let w = load(&args[1], &cfg);
            print!(
                "{}",
                Listing {
                    warrior: &w,
                    core_size: cfg.core_size
                }
            );
        }
        Some("pair") if args.len() >= 3 => {
            let a = load(&args[1], &cfg);
            let b = load(&args[2], &cfg);
            let rounds = cfg.rounds;
            let seed = flag(&args, "--seed").unwrap_or(1) as i32;
            let (a, b) = (Compiled::new(&a), Compiled::new(&b));
            let mut mars = Engine::new(&cfg);
            let t0 = std::time::Instant::now();
            let s = mars.play(&cfg, &a, &b, rounds, seed);
            let dt = t0.elapsed().as_secs_f64();
            println!("Results: {} {} {}", s.w1, s.w2, s.ties);
            eprintln!(
                "{} rounds, {} instructions, {:.3} s, {:.1} M instructions/s",
                rounds,
                mars.steps,
                dt,
                mars.steps as f64 / dt / 1e6
            );
        }
        Some("battle") if args.len() >= 3 => {
            let a = load(&args[1], &cfg);
            let b = load(&args[2], &cfg);
            let pos = flag(&args, "--pos").unwrap_or(cfg.core_size / 2);
            let first = flag(&args, "--first").unwrap_or(0) as usize;
            let (a, b) = (Compiled::new(&a), Compiled::new(&b));
            let mut mars = Engine::new(&cfg);
            let (w1, w2, t) = match mars.battle(&cfg, [&a, &b], [0, pos], first) {
                Outcome::Win(0) => (1, 0, 0),
                Outcome::Win(_) => (0, 1, 0),
                Outcome::Tie => (0, 0, 1),
            };
            println!("Results: {} {} {}", w1, w2, t);
        }
        Some("tournament") if args.len() >= 3 => {
            let files: Vec<&String> = {
                let mut v = Vec::new();
                let mut i = 1;
                while i < args.len() {
                    if args[i].starts_with("--") {
                        i += 2;
                    } else {
                        v.push(&args[i]);
                        i += 1;
                    }
                }
                v
            };
            let second = Config {
                first_warrior: false,
                ..cfg
            };
            let named: Vec<(Warrior, Compiled, Compiled)> = files
                .iter()
                .map(|f| {
                    let w = load(f, &cfg);
                    let c2 = Compiled::new(&load(f, &second));
                    let c1 = Compiled::new(&w);
                    (w, c1, c2)
                })
                .collect();
            let ws: Vec<(&Compiled, &Compiled)> = named.iter().map(|(_, a, b)| (a, b)).collect();
            let mut score = vec![0u64; ws.len()];
            let mut jobs = Vec::new();
            let mut pairs = Vec::new();
            for i in 0..ws.len() {
                for j in i + 1..ws.len() {
                    let k = jobs.len() as u32;
                    let x = 100 + (k * 997 % 7801);
                    jobs.push(Job {
                        a: ws[i].0,
                        b: ws[j].1,
                        seed: (x - cfg.min_distance) as i32,
                    });
                    pairs.push((i, j));
                }
            }
            let lanes = flag(&args, "--lanes").unwrap_or(1);
            let threads = match flag(&args, "--jobs").unwrap_or(1) {
                0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
                n => n as usize,
            };
            if lanes >= 2 && threads > 1 {
                eprintln!("--lanes 2 plays on one thread; drop --jobs");
                exit(2);
            }
            let t0 = std::time::Instant::now();
            let (results, steps): (Vec<Score>, u64) = if lanes >= 2 {
                let mut m = Multi::new(&cfg);
                let r = m.play_all(&cfg, &jobs, cfg.rounds);
                (r, m.steps)
            } else {
                pool::play_all(&cfg, &jobs, cfg.rounds, threads)
            };
            let k = jobs.len();
            for (&(i, j), s) in pairs.iter().zip(&results) {
                println!(
                    "{} vs {}: Results: {} {} {}",
                    files[i], files[j], s.w1, s.w2, s.ties
                );
                score[i] += 3 * s.w1 as u64 + s.ties as u64;
                score[j] += 3 * s.w2 as u64 + s.ties as u64;
            }
            let dt = t0.elapsed().as_secs_f64();
            let mut order: Vec<usize> = (0..ws.len()).collect();
            order.sort_by(|&x, &y| score[y].cmp(&score[x]));
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
                k,
                cfg.rounds,
                steps,
                dt,
                steps as f64 / dt / 1e6
            );
        }
        _ => {
            eprintln!("usage: cw check FILE... | cw list FILE | cw pair A B [--rounds N] [--seed S] | cw battle A B --pos N [--first 0|1]");
            exit(2);
        }
    }
}

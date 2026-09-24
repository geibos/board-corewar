//! `cw` — the hill's command line.
//!
//!   cw check FILE...                 assemble, print name/author/length or the error
//!   cw list FILE                     the assembled program, one instruction per line
//!   cw pair A B [--rounds N] [--seed S]
//!                                    a match of N rounds (default 250), like `pmars -b -r N`
//!   cw battle A B --pos N [--first 0|1]
//!                                    one battle, B loaded at N; prints `Results: W1 W2 T`
//!                                    exactly like `pmars -b -r 1 -F N A B`

use corewar::asm::{assemble, Config};
use corewar::mars::{Mars, Outcome};
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
    let cfg = Config::default();
    match args.first().map(String::as_str) {
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
            let rounds = flag(&args, "--rounds").unwrap_or(250);
            let seed = flag(&args, "--seed").unwrap_or(1) as i32;
            let mut mars = Mars::new(&cfg, 2);
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
            let mut mars = Mars::new(&cfg, 2);
            let (w1, w2, t) = match mars.battle(&cfg, [&a, &b], [0, pos], first) {
                Outcome::Win(0) => (1, 0, 0),
                Outcome::Win(_) => (0, 1, 0),
                Outcome::Tie => (0, 0, 1),
            };
            println!("Results: {} {} {}", w1, w2, t);
        }
        _ => {
            eprintln!("usage: cw check FILE... | cw list FILE | cw pair A B [--rounds N] [--seed S] | cw battle A B --pos N [--first 0|1]");
            exit(2);
        }
    }
}

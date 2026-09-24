//! Differential tests against pMARS, the reference simulator.
//!
//! All of them run only when PMARS points at a pMARS binary built with
//! -DSERVER -DEXT94, and are `#[ignore]`d so a plain `cargo test` stays fast:
//!
//!   export PMARS=$PWD/third_party/pmars/src/pmars
//!   cargo test --release --test pmars_diff -- --ignored            # all three
//!   PROPTEST_CASES=20000 cargo test --release --test pmars_diff same_listing -- --ignored
//!   CW_SOAK_SECS=3600    cargo test --release --test pmars_diff soak -- --ignored --nocapture
//!   CW_CORPUS=~/warriors cargo test --release --test pmars_diff corpus -- --ignored --nocapture
//!
//! * `same_listing_and_outcome_as_pmars` — proptest: random warriors that use
//!   every opcode, modifier and mode; same listing, same battle outcome.
//!   Cycle and process limits are random too, down to tiny ones: a small
//!   cycle limit pins the exact step at which a warrior dies, a small process
//!   limit makes SPL hit its ceiling constantly. Failures are shrunk to a
//!   minimal pair and kept in tests/pmars_diff.regressions.
//! * `soak_against_pmars` — the same check, longer warriors, a fresh random
//!   seed for every batch, for CW_SOAK_SECS seconds (default 600).
//! * `corpus_matches_like_pmars` — real warriors: every pair from testdata/,
//!   pMARS's own warriors/ and CW_CORPUS (colon-separated dirs) plays a full
//!   match (CW_ROUNDS, default 250) against `pmars -r N -F X`, same positions.
//!
//! pMARS silently ignores warrior files with long paths, so warriors are
//! written to a short directory: CW_DIFF_DIR, default /tmp/cwdiff.

mod common;

use common::{render, warrior, warrior_of, Spec};
use corewar::asm::{assemble, Config};
use corewar::mars::{Mars, Outcome};
use corewar::redcode::{signed, Warrior};
use proptest::prelude::*;
use proptest::test_runner::{
    Config as PtConfig, FileFailurePersistence, TestCaseError, TestRunner,
};
use std::process::Command;
use std::time::{Duration, Instant};

fn pmars() -> Option<String> {
    std::env::var("PMARS").ok()
}

fn short_dir() -> String {
    let dir = std::env::var("CW_DIFF_DIR").unwrap_or_else(|_| "/tmp/cwdiff".into());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// pMARS's listing of the first program and its `Results:` line.
fn parse_pmars(out: &str) -> (u32, Vec<String>, String) {
    let result = out
        .lines()
        .find(|l| l.starts_with("Results:"))
        .unwrap_or("NO RESULT")
        .to_string();
    let mut code = Vec::new();
    let mut start = 0;
    let mut lines = out.lines().skip_while(|l| !l.starts_with("Program "));
    lines.next();
    for l in lines {
        let l = l.trim_end();
        if l.starts_with("Program ") || l.contains(" scores ") || l.starts_with("Results") {
            break;
        }
        let t = l.trim_start();
        if t.is_empty() || t.starts_with("ORG") {
            continue;
        }
        let t = match t.strip_prefix("START") {
            Some(rest) => {
                start = code.len() as u32;
                rest.trim_start()
            }
            None => t,
        };
        // "MOV.I  $     0, $     1"
        let Some((opm, rest)) = t.split_once(char::is_whitespace) else {
            continue;
        };
        let Some((a, b)) = rest.split_once(',') else {
            continue;
        };
        let norm = |x: &str| x.split_whitespace().collect::<String>();
        code.push(format!(
            "{} {}, {}",
            opm.replace("CMP", "SEQ"),
            norm(a),
            norm(b)
        ));
    }
    (start, code, result)
}

fn our_listing(w: &Warrior, cs: u32) -> Vec<String> {
    w.code
        .iter()
        .map(|i| {
            format!(
                "{}.{} {}{}, {}{}",
                i.op.name(),
                i.modifier.name(),
                i.a_mode.symbol(),
                signed(i.a, cs),
                i.b_mode.symbol(),
                signed(i.b, cs)
            )
        })
        .collect()
}

fn cycles() -> impl Strategy<Value = u32> {
    prop_oneof![1 => 1u32..50, 1 => 1u32..3000, 2 => Just(80_000u32)]
}

fn processes() -> impl Strategy<Value = u32> {
    prop_oneof![1 => 1u32..8, 1 => 1u32..200, 1 => Just(8000u32)]
}

type Case = ((Vec<Spec>, usize), (Vec<Spec>, usize), u32, u32, u32);

/// One case: both assemblers, one battle, compared.
fn check(
    pmars: &str,
    ((a, sa), (b, sb), pos, max_cycles, max_processes): Case,
) -> Result<(), TestCaseError> {
    let dir = short_dir();
    let (src_a, src_b) = (render("a", &a, sa), render("b", &b, sb));
    let id = std::process::id();
    let (pa, pb) = (
        format!("{}/a{}.red", dir, id),
        format!("{}/b{}.red", dir, id),
    );
    std::fs::write(&pa, &src_a).unwrap();
    std::fs::write(&pb, &src_b).unwrap();

    let mut cfg = Config::default();
    cfg.max_cycles = max_cycles;
    cfg.max_processes = max_processes;
    let wa = assemble(&src_a, &cfg).unwrap();
    let wb = assemble(&src_b, &cfg).unwrap();

    let out = Command::new(pmars)
        .args([
            "-r",
            "1",
            "-F",
            &pos.to_string(),
            "-c",
            &max_cycles.to_string(),
            "-p",
            &max_processes.to_string(),
            &pa,
            &pb,
        ])
        .output()
        .expect("run pmars");
    let (their_start, their_code, their_result) =
        parse_pmars(&String::from_utf8_lossy(&out.stdout));

    prop_assert_eq!(
        our_listing(&wa, cfg.core_size),
        their_code,
        "listing of\n{}",
        src_a
    );
    prop_assert_eq!(wa.start, their_start);
    let ours = match Mars::new(&cfg, 2).battle(&cfg, [&wa, &wb], [0, pos], 0) {
        Outcome::Win(0) => "Results: 1 0 0",
        Outcome::Win(_) => "Results: 0 1 0",
        Outcome::Tie => "Results: 0 0 1",
    };
    prop_assert_eq!(
        ours,
        their_result.as_str(),
        "--- a ---\n{}--- b ---\n{}",
        src_a,
        src_b
    );
    Ok(())
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000)
}

fn persistence() -> Option<Box<dyn proptest::test_runner::FailurePersistence>> {
    Some(Box::new(FileFailurePersistence::WithSource("regressions")))
}

proptest! {
    #![proptest_config(PtConfig { cases: cases(), failure_persistence: persistence(), max_shrink_iters: 4000, ..PtConfig::default() })]

    #[test]
    #[ignore]
    fn same_listing_and_outcome_as_pmars(
        a in warrior(), b in warrior(), pos in 100u32..7880, c in cycles(), p in processes(),
    ) {
        let Some(pm) = pmars() else { return Ok(()); };
        check(&pm, (a, b, pos, c, p))?;
    }
}

#[test]
#[ignore]
fn soak_against_pmars() {
    let Some(pm) = pmars() else {
        eprintln!("PMARS is not set; skipping");
        return;
    };
    let secs: u64 = std::env::var("CW_SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let deadline = Instant::now() + Duration::from_secs(secs);
    let strategy = (
        warrior_of(40),
        warrior_of(40),
        100u32..7840,
        cycles(),
        processes(),
    );
    let (mut batches, mut total) = (0u32, 0u64);
    while Instant::now() < deadline {
        // TestRunner::new seeds itself randomly: every batch explores new ground.
        let mut runner = TestRunner::new(PtConfig {
            cases: 250,
            failure_persistence: persistence(),
            max_shrink_iters: 4000,
            source_file: Some(file!()),
            ..PtConfig::default()
        });
        if let Err(e) = runner.run(&strategy, |case| check(&pm, case)) {
            panic!("after {} identical battles: {}", total, e);
        }
        batches += 1;
        total += 250;
    }
    eprintln!(
        "soak: {} batches, {} battles identical to pMARS in {} s",
        batches, total, secs
    );
}

fn corpus_files() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> =
        vec!["testdata".into(), "third_party/pmars/warriors".into()];
    if let Ok(extra) = std::env::var("CW_CORPUS") {
        dirs.extend(
            extra
                .split(':')
                .filter(|d| !d.is_empty())
                .map(std::path::PathBuf::from),
        );
    }
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "red").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
#[ignore]
fn corpus_matches_like_pmars() {
    let Some(pm) = pmars() else {
        eprintln!("PMARS is not set; skipping");
        return;
    };
    let rounds: u32 = std::env::var("CW_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250);
    let cfg = Config::default();
    let dir = short_dir();
    let mut warriors = Vec::new();
    for p in corpus_files() {
        let src = std::fs::read_to_string(&p).unwrap_or_default();
        match assemble(&src, &cfg) {
            Ok(w) => {
                let short = format!("{}/c{}.red", dir, warriors.len());
                std::fs::write(&short, &src).unwrap();
                warriors.push((p.display().to_string(), short, w));
            }
            Err(e) => eprintln!("skip {}: {}", p.display(), e),
        }
    }
    assert!(warriors.len() >= 2, "need at least two warriors");
    let mut mars = Mars::new(&cfg, 2);
    let mut pairs = 0;
    for (i, (na, fa, wa)) in warriors.iter().enumerate() {
        for (nb, fb, wb) in warriors.iter().skip(i + 1) {
            // pmars -F X seeds its position generator with X - separation.
            let x = 100 + (pairs * 997 % 7801) as u32;
            let s = mars.play(&cfg, wa, wb, rounds, (x - cfg.min_distance) as i32);
            let ours = format!("Results: {} {} {}", s.w1, s.w2, s.ties);
            let out = Command::new(&pm)
                .args([
                    "-b",
                    "-r",
                    &rounds.to_string(),
                    "-F",
                    &x.to_string(),
                    fa,
                    fb,
                ])
                .output()
                .expect("run pmars");
            let theirs = String::from_utf8_lossy(&out.stdout)
                .lines()
                .find(|l| l.starts_with("Results:"))
                .unwrap_or("NO RESULT")
                .to_string();
            assert_eq!(
                ours, theirs,
                "{} vs {}, {} rounds, -F {}",
                na, nb, rounds, x
            );
            pairs += 1;
        }
    }
    eprintln!(
        "corpus: {} warriors, {} pairs x {} rounds identical to pMARS",
        warriors.len(),
        pairs,
        rounds
    );
}

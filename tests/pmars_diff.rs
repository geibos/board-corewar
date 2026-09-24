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
//! * `cli_parameters_like_pmars` — proptest: `cw pair` and `pmars -b` with
//!   random -s -c -p -l -d, invalid sets included: both refuse, or both
//!   print the same Results line.
//!
//! pMARS silently ignores warrior files with long paths, so warriors are
//! written to a short directory: CW_DIFF_DIR, default /tmp/cwdiff.

mod common;

use common::source::source;
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

/// pMARS's listing of the first program and its `Results:` line. The start
/// is the line pMARS labels START; it labels none when execution starts
/// outside the program (a warning), hence the Option.
fn parse_pmars(out: &str) -> (Option<u32>, Vec<String>, String) {
    let result = out
        .lines()
        .find(|l| l.starts_with("Results:"))
        .unwrap_or("NO RESULT")
        .to_string();
    let mut code = Vec::new();
    let mut start = None;
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
                start = Some(code.len() as u32);
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
        code.push(format!("{} {}, {}", opm, norm(a), norm(b)));
    }
    (start, code, result)
}

/// The start as pMARS's listing shows it: none when outside the program.
fn listed_start(w: &Warrior) -> Option<u32> {
    ((w.start as usize) < w.code.len()).then_some(w.start)
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
    // Unique per call: cargo runs tests on parallel threads of one process,
    // and two tests sharing a file name compared each other's warriors.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!(
        "{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let (pa, pb) = (
        format!("{}/a{}.red", dir, id),
        format!("{}/b{}.red", dir, id),
    );
    std::fs::write(&pa, &src_a).unwrap();
    std::fs::write(&pb, &src_b).unwrap();

    let cfg = Config {
        max_cycles,
        max_processes,
        ..Config::default()
    };
    let wa = assemble(&src_a, &cfg).unwrap();
    let wb = assemble(
        &src_b,
        &Config {
            first_warrior: false,
            ..cfg
        },
    )
    .unwrap();

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
    let _ = std::fs::remove_file(&pa);
    let _ = std::fs::remove_file(&pb);

    prop_assert_eq!(
        our_listing(&wa, cfg.core_size),
        their_code,
        "listing of\n{}",
        src_a
    );
    prop_assert_eq!(listed_start(&wa), their_start);
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

/// One source: pMARS and this assembler must agree on accepting or
/// rejecting it, and when both accept, on the listing and the start.
fn check_source(pmars: &str, src: &str) -> Result<(), TestCaseError> {
    let dir = short_dir();
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!(
        "{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let path = format!("{}/s{}.red", dir, id);
    std::fs::write(&path, src).unwrap();
    let out = Command::new(pmars)
        .args(["-r", "1", "-F", "4000", &path, "testdata/imp.red"])
        .output()
        .expect("run pmars");
    let _ = std::fs::remove_file(&path);
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    // Accepted means pMARS went on to fight: with errors it prints "Number of
    // errors", or, past its error limit, only "Program aborted".
    let theirs_ok = text.contains("Results:");
    let ours = assemble(src, &Config::default());
    prop_assert_eq!(
        ours.is_ok(),
        theirs_ok,
        "accept/reject differs; ours: {:?}\n--- source ---\n{}\n--- pmars ---\n{}",
        ours.as_ref().err(),
        src,
        text
    );
    if let Ok(w) = ours {
        let (start, code, _) = parse_pmars(&text);
        prop_assert_eq!(our_listing(&w, 8000), code, "listing of\n{}", src);
        prop_assert_eq!(listed_start(&w), start, "start of\n{}", src);
    }
    Ok(())
}

proptest! {
    #![proptest_config(PtConfig { cases: cases(), failure_persistence: persistence(), max_shrink_iters: 8000, ..PtConfig::default() })]

    #[test]
    #[ignore]
    fn assembler_matches_pmars_on_sources(src in source()) {
        let Some(pm) = pmars() else { return Ok(()); };
        check_source(&pm, &src)?;
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
    // ROUNDS is visible to warriors (pspace.red asserts ROUNDS > 1): assemble
    // with the match's own number of rounds, as pmars -r N does.
    let cfg = Config {
        rounds,
        ..Config::default()
    };
    let dir = short_dir();
    let mut warriors = Vec::new();
    for p in corpus_files() {
        let src = std::fs::read_to_string(&p).unwrap_or_default();
        // Assembled twice: as pMARS's first warrior and as its second.
        let second = Config {
            first_warrior: false,
            ..cfg
        };
        match (assemble(&src, &cfg), assemble(&src, &second)) {
            (Ok(w1), Ok(w2)) => {
                let short = format!("{}/c{}.red", dir, warriors.len());
                std::fs::write(&short, &src).unwrap();
                warriors.push((p.display().to_string(), short, w1, w2));
            }
            (Err(e), _) | (_, Err(e)) => eprintln!("skip {}: {}", p.display(), e),
        }
    }
    assert!(warriors.len() >= 2, "need at least two warriors");
    let mut mars = Mars::new(&cfg, 2);
    let mut pairs = 0;
    for (i, (na, fa, wa, _)) in warriors.iter().enumerate() {
        for (nb, fb, _, wb) in warriors.iter().skip(i + 1) {
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

/// How much the source generator exercises: the share of sources the
/// assembler accepts, and of those, how many use each feature. A diff test
/// that only ever compares rejections proves little.
#[test]
#[ignore]
fn source_generator_coverage() {
    use proptest::strategy::ValueTree;
    let mut runner = TestRunner::deterministic();
    let n = 3000;
    let mut accepted = 0;
    let feats = [
        "for ", "&ct", "equ ", "CURLINE", "(r=", "(s=", "(t=", "\\\n", "org ", "pin ", "end ",
        "mc0", "mc1",
    ];
    let mut used = vec![0usize; feats.len()];
    for _ in 0..n {
        let src = source().new_tree(&mut runner).unwrap().current();
        if assemble(&src, &Config::default()).is_ok() {
            accepted += 1;
            for (k, f) in feats.iter().enumerate() {
                if src.contains(f) {
                    used[k] += 1;
                }
            }
        }
    }
    eprintln!("accepted {} of {}", accepted, n);
    for (f, u) in feats.iter().zip(used) {
        eprintln!(
            "  {:10} in {:4} accepted sources",
            f.escape_debug().to_string(),
            u
        );
    }
}

/// Real warriors for the command-line checks, copied to the short directory.
fn cli_warriors() -> Vec<String> {
    let dir = short_dir();
    let mut out = Vec::new();
    let mut files: Vec<String> = ["dwarf", "edge", "imp", "registers"]
        .iter()
        .map(|n| format!("testdata/{}.red", n))
        .collect();
    for n in ["aeka", "flashpaper", "pspace", "rave", "validate"] {
        let p = format!("third_party/pmars/warriors/{}.red", n);
        if std::path::Path::new(&p).exists() {
            files.push(p);
        }
    }
    for f in files {
        let short = format!("{}/cli_{}", dir, f.rsplit('/').next().unwrap());
        std::fs::copy(&f, &short).unwrap();
        out.push(short);
    }
    out
}

/// Match parameters as flags: `None` leaves a flag out.
#[derive(Debug, Clone)]
struct Params {
    s: Option<u32>,
    c: Option<u32>,
    p: Option<u32>,
    l: Option<u32>,
    d: Option<u32>,
    rounds: u32,
    /// Position of warrior #2 past the distance: -F distance + k.
    k: u32,
}

fn params() -> impl Strategy<Value = Params> {
    // Weighted towards valid sets, so that most cases play a match; the
    // rest check that both refuse the same ones.
    let s = prop_oneof![
        2 => Just(None),
        1 => (2u32..400).prop_map(Some),
        4 => (400u32..8000).prop_map(Some),
        2 => (8000u32..=65535).prop_map(Some),
    ];
    let c = prop_oneof![1 => Just(None), 1 => (1u32..20_000).prop_map(Some)];
    let p = prop_oneof![1 => Just(None), 1 => (1u32..100).prop_map(Some)];
    let l = prop_oneof![
        4 => Just(None),
        1 => (1u32..12).prop_map(Some),
        1 => (1u32..=1001).prop_map(Some),
    ];
    let d = prop_oneof![
        4 => Just(None),
        1 => (1u32..60).prop_map(Some),
        1 => (100u32..3000).prop_map(Some),
    ];
    (s, c, p, l, d, 1u32..6, 0u32..20_000).prop_map(|(s, c, p, l, d, rounds, k)| Params {
        s,
        c,
        p,
        l,
        d,
        rounds,
        k,
    })
}

proptest! {
    #![proptest_config(PtConfig { cases: cases() / 4, failure_persistence: persistence(), ..PtConfig::default() })]

    /// `cw pair` with pMARS's parameter flags: refused by both, or the same
    /// Results line. Parameters are random, invalid combinations included;
    /// a warrior longer than -l is refused by both as well.
    #[test]
    #[ignore]
    fn cli_parameters_like_pmars(pr in params(), a in 0usize..9, b in 0usize..9) {
        let Some(pm) = pmars() else { return Ok(()); };
        let ws = cli_warriors();
        let (fa, fb) = (&ws[a % ws.len()], &ws[b % ws.len()]);
        let distance = pr.d.or(pr.l).unwrap_or(100);
        let x = distance + pr.k;
        let mut flags: Vec<String> = Vec::new();
        for (name, v) in [("-s", pr.s), ("-c", pr.c), ("-p", pr.p), ("-l", pr.l), ("-d", pr.d)] {
            if let Some(v) = v {
                flags.push(name.into());
                flags.push(v.to_string());
            }
        }
        let theirs = Command::new(&pm)
            .args(["-b", "-r", &pr.rounds.to_string(), "-F", &x.to_string()])
            .args(&flags)
            .args([fa, fb])
            .output()
            .expect("run pmars");
        let theirs = String::from_utf8_lossy(&theirs.stdout)
            .lines()
            .find(|l| l.starts_with("Results:"))
            .map(str::to_owned);
        let ours = Command::new(env!("CARGO_BIN_EXE_cw"))
            .args(["pair", fa, fb, "--rounds", &pr.rounds.to_string(), "--seed", &pr.k.to_string()])
            .args(&flags)
            .output()
            .expect("run cw");
        let ours = ours
            .status
            .success()
            .then(|| String::from_utf8_lossy(&ours.stdout).trim_end().to_owned());
        prop_assert_eq!(ours, theirs, "{} {} {:?} -F {}", fa, fb, flags, x);
    }
}

/// Every match a hill plays, replayed by pMARS: the warrior whose id sorts
/// first is pMARS's first, and -F is the seed plus the distance.
#[test]
#[ignore]
fn hill_matches_like_pmars() {
    let Some(pm) = pmars() else {
        eprintln!("PMARS is not set; skipping");
        return;
    };
    let dir = format!("{}/hill_{}", short_dir(), std::process::id());
    let _ = std::fs::remove_dir_all(&dir);
    let cw = env!("CARGO_BIN_EXE_cw");
    let run = |args: &[&str]| {
        let o = Command::new(cw).args(args).output().expect("run cw");
        assert!(o.status.success(), "{:?}", o);
        o.stdout
    };
    run(&["hill", "init", &dir, "--size", "0", "--rounds", "50"]);
    let files: Vec<String> = corpus_files()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    let mut args = vec!["hill", "challenge", dir.as_str(), "--json"];
    args.extend(files.iter().map(String::as_str));
    let v: serde_json::Value = serde_json::from_slice(&run(&args)).unwrap();
    let played = v["played"].as_array().unwrap();
    assert!(played.len() >= 28, "{} matches", played.len());
    for m in played {
        let (a, b) = (m["a"].as_str().unwrap(), m["b"].as_str().unwrap());
        let x = m["seed"].as_u64().unwrap() + 100;
        let out = Command::new(&pm)
            .args(["-b", "-r", "50", "-F", &x.to_string()])
            .arg(format!("{}/warriors/{}.red", dir, a))
            .arg(format!("{}/warriors/{}.red", dir, b))
            .output()
            .expect("run pmars");
        let theirs = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.starts_with("Results:"))
            .unwrap_or("NO RESULT")
            .to_string();
        let r = &m["result"];
        assert_eq!(
            format!("Results: {} {} {}", r["w1"], r["w2"], r["ties"]),
            theirs,
            "{} vs {}, -F {}",
            a,
            b,
            x
        );
    }
    eprintln!("hill: {} matches identical to pMARS", played.len());
    let _ = std::fs::remove_dir_all(&dir);
}

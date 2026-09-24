//! Untrusted sources: the hill assembles whatever agents send, so the
//! assembler must answer every input — accept or reject — without a panic,
//! a stack overflow, or unbounded time. Cases found by probing
//! (scripts/hostile-probes.py) are pinned here; the expected verdicts are
//! pMARS's where pMARS gives one.

use corewar::asm::{assemble, AsmError, Config, MAX_SOURCE_BYTES};
use corewar::redcode::Warrior;
use std::time::{Duration, Instant};

/// Time allowed for one source: a detector of hangs and blow-ups (the
/// cases below took seconds to hours before), not a benchmark. CI runners
/// and musl builds are several times slower than a desktop, and tests run
/// side by side; debug builds are slower again. Growth rates are checked
/// separately, by ratio.
fn budget() -> Duration {
    Duration::from_secs(if cfg!(debug_assertions) { 60 } else { 10 })
}

/// Assemble, failing if it takes longer than the budget instead of hanging
/// the test run.
fn assemble_bounded(src: &str) -> Result<Warrior, AsmError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let owned = src.to_string();
    std::thread::Builder::new()
        .spawn(move || {
            let t = Instant::now();
            let r = assemble(&owned, &Config::default());
            let _ = tx.send((r, t.elapsed()));
        })
        .unwrap();
    let (r, took) = rx
        .recv_timeout(budget() * 5)
        .unwrap_or_else(|_| panic!("no answer in {:?}", budget() * 5));
    assert!(took <= budget(), "took {:?}", took);
    r
}

fn error_of(src: &str) -> String {
    match assemble_bounded(src) {
        Ok(w) => panic!("accepted, {} instructions", w.code.len()),
        Err(e) => e.msg,
    }
}

/// Non-ASCII text used to panic: a token was taken one byte at a time and
/// the rest of the line sliced from inside a UTF-8 character. pMARS reads
/// bytes and rejects every one of these but the comment and the ;assert.
#[test]
fn non_ascii_text_is_rejected_like_pmars_without_a_panic() {
    let cases = [
        (";name Воин\n;author Автор\nDAT 0, 0 ; комментарий\n", true),
        (";assert ж\nDAT 0,0\n", true),
        (";redcodeж\nDAT 0,0\n", true),
        ("метка DAT 0, 0\nJMP метка\n", false),
        ("DAT ж, 0\n", false),
        ("x EQU ж+1\nDAT x, 0\n", false),
        ("FOR ж\nDAT 0,0\nROF\n", false),
        ("DAT 0,0\nEND ж\n", false),
        ("ORG ж\nDAT 0,0\n", false),
        ("MOV ж0, 1\n", false),
        ("MOV.ж 0, 1\n", false),
        ("😀 DAT 😀, 😀\n", false),
        (&format!(";{}жжж\nDAT 0,0\n", "a".repeat(253)), false),
        (&format!(";{}жжж\nDAT 0,0\n", "a".repeat(254)), false),
    ];
    for (src, accepted) in cases {
        let got = assemble(src, &Config::default());
        assert_eq!(got.is_ok(), accepted, "{:?}: {:?}", src, got.err());
    }
}

/// Deep nesting recursed once per level and overflowed the stack near
/// 17 000 levels on an 8 MiB stack (pMARS itself crashes near 100 000).
/// pMARS accepts these 20 000 levels; here the limit is 10 000.
#[test]
fn deep_for_nesting_is_an_error_not_a_stack_overflow() {
    let n = 20_000;
    let src = "FOR 1\n".repeat(n) + "DAT 0,0\n" + &"ROF\n".repeat(n);
    assert_eq!(error_of(&src), "nesting too deep");
}

#[test]
fn deep_equ_chain_is_an_error_not_a_stack_overflow() {
    let n = 20_000;
    let mut src = String::from("e0 EQU 1\n");
    for k in 1..n {
        src += &format!("e{} EQU e{}\n", k, k - 1);
    }
    src += &format!("DAT e{}, 0\n", n - 1);
    assert_eq!(error_of(&src), "nesting too deep");
}

/// Nesting up to the limit (10 000) still assembles, like pMARS, whatever
/// the caller's stack: the assembler runs on a thread with its own.
#[test]
fn nesting_below_the_limit_still_assembles() {
    let n = 9_000;
    let src = "FOR 1\n".repeat(n) + "DAT 0,0\n" + &"ROF\n".repeat(n);
    assert_eq!(assemble_bounded(&src).unwrap().code.len(), 1);
    let mut src = String::from("e0 EQU 1\n");
    for k in 1..n {
        src += &format!("e{} EQU e{}\n", k, k - 1);
    }
    src += &format!("DAT e{}, 0\n", n - 1);
    assert_eq!(assemble_bounded(&src).unwrap().code.len(), 1);
}

/// FOR loops that produce nothing ran every round: two nested FOR 65535
/// are 4.3e9 rounds, which hangs pMARS too.
#[test]
fn empty_nested_for_loops_are_bounded() {
    let src = "FOR 65535\nFOR 65535\nROF\nROF\nDAT 0,0\n";
    assert_eq!(error_of(src), "too much FOR expansion");
    let src = "FOR 65535\nFOR 65535\nFOR 65535\n;x\nROF\nROF\nROF\nDAT 0,0\n";
    assert_eq!(error_of(src), "too much FOR expansion");
}

fn equs(n: usize) -> String {
    let equs: String = (0..n).map(|k| format!("l{} EQU {}\n", k, k)).collect();
    equs + &format!("DAT l{}, 0\n", n - 1)
}

/// Fastest of three: the least disturbed by other tests running alongside.
fn fastest(src: &str) -> Duration {
    (0..3)
        .map(|_| {
            let t = Instant::now();
            let _ = assemble(src, &Config::default());
            t.elapsed()
        })
        .min()
        .unwrap()
}

/// Symbol lookup scanned every symbol: 40 000 labels took 4 s, and four
/// times as many symbols took sixteen times as long.
#[test]
fn many_labels_and_equs_assemble_in_time() {
    // As many as fit in MAX_SOURCE_BYTES.
    let n = 45_000;
    let labels: String = (0..n).map(|k| format!("l{} DAT {}, 0\n", k, k)).collect();
    assert_eq!(error_of(&labels), "program too long");
    let w = assemble_bounded(&equs(n)).unwrap();
    assert_eq!(w.code.len(), 1);
    let (small, large) = (fastest(&equs(n / 4)), fastest(&equs(n)));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio < 10.0,
        "4x the symbols took {:.1}x as long ({:?} vs {:?}): lookup is not linear",
        ratio,
        large,
        small
    );
}

#[test]
fn a_source_over_the_size_limit_is_rejected() {
    // Comment lines of 64 bytes (pMARS reads a line 255 bytes at a time,
    // so one long comment would continue as code).
    let comment = ";".to_string() + &"x".repeat(62) + "\n";
    let code = "DAT 0,0\n";
    let fits = (MAX_SOURCE_BYTES - code.len()) / comment.len();
    let small = comment.repeat(fits) + code;
    assert!(small.len() <= MAX_SOURCE_BYTES);
    assert!(assemble_bounded(&small).is_ok());
    let big = comment.repeat(fits + 1) + code;
    assert!(big.len() > MAX_SOURCE_BYTES);
    assert_eq!(error_of(&big), "source too large");
}

mod garbage {
    use super::assemble_bounded;
    use proptest::prelude::*;

    /// Fragments that stress the preprocessor and the evaluator, mixed with
    /// arbitrary text: FOR/ROF with any count, EQUs that refer to each other,
    /// `&` concatenation, CURLINE, registers, deep parentheses, non-ASCII.
    fn fragment() -> impl Strategy<Value = String> {
        let name = prop::sample::select(vec!["a", "b", "c", "x1", "lbl", "e"]);
        prop_oneof![
            (0u32..70_000).prop_map(|n| format!("FOR {}", n)),
            Just("ROF".to_string()),
            (name.clone(), name.clone()).prop_map(|(a, b)| format!("{} EQU {}+{}", a, b, b)),
            name.clone().prop_map(|a| format!("{} EQU", a)),
            Just("EQU DAT 1, 1".to_string()),
            (name.clone(), name.clone()).prop_map(|(a, b)| format!("{}&{} DAT {}, 0", a, b, a)),
            name.clone()
                .prop_map(|a| format!("{} MOV.I {}, CURLINE", a, a)),
            (1usize..130).prop_map(|d| format!("DAT {}1{}, z=z+1", "(".repeat(d), ")".repeat(d))),
            Just(";assert a == a".to_string()),
            Just(";redcode".to_string()),
            prop::sample::select(vec!["END a", "ORG b", "PIN 7", "END", "CURLINE EQU 1"])
                .prop_map(String::from),
            "[ -~]{0,300}",
            "\\PC{0,40}",
            "\\PC{0,20}".prop_map(|t| format!("MOV {}, 0", t)),
            "\\PC{0,20}".prop_map(|t| format!("FOR 2+{}", t)),
            (name, 0usize..40).prop_map(|(a, n)| format!(
                "DAT {}{}",
                format!("{}+", a).repeat(n),
                a
            )),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

        #[test]
        fn any_source_gets_an_answer_in_time(lines in prop::collection::vec(fragment(), 0..60)) {
            let src = lines.join("\n") + "\n";
            let _ = assemble_bounded(&src);
        }
    }
}

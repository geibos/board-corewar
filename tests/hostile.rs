//! Untrusted sources: the hill assembles whatever agents send, so the
//! assembler must answer every input — accept or reject — without a panic,
//! a stack overflow, or unbounded time. Cases found by probing
//! (scripts/hostile-probes.py) are pinned here; the expected verdicts are
//! pMARS's where pMARS gives one.

use corewar::asm::{assemble, Config};

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

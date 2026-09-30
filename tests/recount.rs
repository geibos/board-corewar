//! `cw hill recount`: the hill's members replayed on fresh placements from
//! a given text, each pair `--runs` times, for a final table nobody could
//! fit a warrior to. The hill itself is not changed.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cw(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cw"))
        .args(args)
        .output()
        .expect("run cw")
}

fn json(args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.push("--json");
    let o = cw(&all);
    assert!(o.status.success(), "{:?}", o);
    serde_json::from_slice(&o.stdout).expect("stdout is one JSON document")
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn hill(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cw-recount-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    let h = d.join("hill");
    assert!(cw(&["hill", "init", s(&h), "--rounds", "20"])
        .status
        .success());
    let o = cw(&[
        "hill",
        "challenge",
        s(&h),
        "testdata/dwarf.red",
        "testdata/imp.red",
        "testdata/edge.red",
    ]);
    assert!(o.status.success(), "{:?}", o);
    h
}

/// The documented seed of run k of a pair.
fn recount_seed(text: &str, a: &str, b: &str, k: u64, positions: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("{}:{}:{}:{}", text, a, b, k).as_bytes());
    u64::from_be_bytes(d[..8].try_into().unwrap()) % positions
}

#[test]
fn every_pair_plays_every_run_as_cw_pair_would() {
    let h = hill("pairs");
    let v = json(&["hill", "recount", s(&h), "--seed", "abc", "--runs", "3"]);
    assert_eq!(v["seed"], "abc");
    assert_eq!(v["runs"], 3);
    let ms = v["matches"].as_array().unwrap();
    assert_eq!(ms.len(), 3 * 3);
    for m in ms {
        let (a, b) = (m["a"].as_str().unwrap(), m["b"].as_str().unwrap());
        assert!(a < b, "the smaller id moves first");
        let k = m["run"].as_u64().unwrap();
        let seed = recount_seed("abc", a, b, k, 8001 - 200);
        assert_eq!(m["seed"].as_u64(), Some(seed));
        let pair = json(&[
            "pair",
            s(&h.join("warriors").join(format!("{}.red", a))),
            s(&h.join("warriors").join(format!("{}.red", b))),
            "--rounds",
            "20",
            "--seed",
            &seed.to_string(),
        ]);
        assert_eq!(m["result"], pair["result"], "{} {} run {}", a, b, k);
    }
}

#[test]
fn the_table_adds_up_every_run() {
    let h = hill("table");
    let v = json(&["hill", "recount", s(&h), "--seed", "abc", "--runs", "3"]);
    let ms = v["matches"].as_array().unwrap();
    let st = v["standings"].as_array().unwrap();
    assert_eq!(st.len(), 3);
    let mut prev = i64::MAX;
    for (i, row) in st.iter().enumerate() {
        assert_eq!(row["place"], i + 1);
        let id = row["id"].as_str().unwrap();
        let (mut w, mut t, mut l) = (0, 0, 0);
        for m in ms {
            let r = &m["result"];
            if m["a"] == id {
                w += r["w1"].as_i64().unwrap();
                l += r["w2"].as_i64().unwrap();
            } else if m["b"] == id {
                w += r["w2"].as_i64().unwrap();
                l += r["w1"].as_i64().unwrap();
            } else {
                continue;
            }
            t += r["ties"].as_i64().unwrap();
        }
        assert_eq!(
            (
                row["wins"].as_i64(),
                row["ties"].as_i64(),
                row["losses"].as_i64()
            ),
            (Some(w), Some(t), Some(l))
        );
        let score = row["score"].as_i64().unwrap();
        assert_eq!(score, 3 * w + t);
        assert!(score <= prev);
        prev = score;
    }
}

#[test]
fn the_same_text_gives_the_same_table_and_another_text_other_placements() {
    let h = hill("same");
    let one = json(&["hill", "recount", s(&h), "--seed", "abc", "--runs", "2"]);
    let two = json(&["hill", "recount", s(&h), "--seed", "abc", "--runs", "2"]);
    let other = json(&["hill", "recount", s(&h), "--seed", "abd", "--runs", "2"]);
    assert_eq!(one["matches"], two["matches"]);
    assert_eq!(one["standings"], two["standings"]);
    let seeds = |v: &Value| -> Vec<Value> {
        v["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["seed"].clone())
            .collect()
    };
    assert_ne!(seeds(&one), seeds(&other));
}

#[test]
fn the_hill_is_left_as_it_was() {
    let h = hill("untouched");
    let read = |f: &str| std::fs::read(h.join(f)).unwrap();
    let before: Vec<Vec<u8>> = ["hill.toml", "state.json", "results.json", "history.jsonl"]
        .iter()
        .map(|f| read(f))
        .collect();
    json(&["hill", "recount", s(&h), "--seed", "abc", "--runs", "2"]);
    let after: Vec<Vec<u8>> = ["hill.toml", "state.json", "results.json", "history.jsonl"]
        .iter()
        .map(|f| read(f))
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_seed_and_at_least_one_run_are_needed() {
    let h = hill("args");
    for (args, why) in [
        (vec!["hill", "recount", s(&h), "--runs", "2"], "--seed"),
        (
            vec!["hill", "recount", s(&h), "--seed", "abc", "--runs", "0"],
            "--runs",
        ),
        (
            vec!["hill", "recount", s(&h), "--seed", "", "--runs", "2"],
            "seed",
        ),
    ] {
        let o = cw(&args);
        assert_eq!(o.status.code(), Some(2), "{:?}: {:?}", args, o);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            err.contains(why) && !err.contains("unrecognized"),
            "{:?}: {}",
            args,
            err
        );
    }
}

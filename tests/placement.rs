//! `placement` in hill.toml: "hash" (the default) places every match from
//! the two warriors' ids; "random" from a number drawn at each challenge,
//! kept with the results so that `verify` and anyone with the output can
//! replay it.

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

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cw-placement-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

const TRIO: [&str; 3] = [
    "testdata/dwarf.red",
    "testdata/imp.red",
    "testdata/edge.red",
];

fn hill(name: &str, placement: Option<&str>) -> PathBuf {
    let d = scratch(name).join("hill");
    let mut args = vec!["hill", "init", s(&d), "--rounds", "20"];
    if let Some(p) = placement {
        args.extend(["--placement", p]);
    }
    let o = cw(&args);
    assert!(o.status.success(), "{:?}", o);
    d
}

fn challenge(dir: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["hill", "challenge", s(dir)];
    args.extend(TRIO);
    args.extend_from_slice(extra);
    json(&args)
}

/// The documented pair seed of a random hill.
fn drawn(draw: u64, a: &str, b: &str, positions: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("{}:{}:{}", draw, a, b).as_bytes());
    u64::from_be_bytes(d[..8].try_into().unwrap()) % positions
}

#[test]
fn a_hash_hill_keeps_its_files_as_they_were() {
    let h = hill("hash", None);
    assert!(!read(&h.join("hill.toml")).contains("placement"));
    let r = challenge(&h, &[]);
    assert!(r.get("seed").is_none(), "{}", r);
    assert!(!read(&h.join("results.json")).contains("seeds"));
    assert!(!read(&h.join("history.jsonl")).contains("seed"));
}

#[test]
fn a_seed_is_refused_on_a_hash_hill() {
    let h = hill("refused", None);
    let mut args = vec!["hill", "challenge", s(&h)];
    args.extend(TRIO);
    args.extend(["--seed", "42"]);
    let o = cw(&args);
    assert_eq!(o.status.code(), Some(2), "{:?}", o);
    assert!(String::from_utf8_lossy(&o.stderr).contains("placement"));
}

#[test]
fn a_random_hill_draws_a_seed_and_keeps_every_matchs() {
    let h = hill("random", Some("random"));
    assert!(read(&h.join("hill.toml")).contains("placement = \"random\""));
    let r = challenge(&h, &[]);
    let draw = r["seed"].as_u64().expect("the report names the drawn seed");
    let results: Value = serde_json::from_str(&read(&h.join("results.json"))).unwrap();
    let played = r["played"].as_array().unwrap();
    assert_eq!(played.len(), 3);
    for p in played {
        let (a, b) = (p["a"].as_str().unwrap(), p["b"].as_str().unwrap());
        assert_eq!(p["seed"].as_u64(), Some(drawn(draw, a, b, 8001 - 200)));
        assert_eq!(results["seeds"][format!("{}:{}", a, b)], p["seed"]);
    }
    assert!(read(&h.join("history.jsonl")).contains(&format!("\"seed\":{}", draw)));
    let v = json(&["hill", "verify", s(&h)]);
    assert_eq!(v["ok"], true, "{}", v);
}

#[test]
fn the_same_seed_makes_the_same_hill() {
    let (one, two) = (
        hill("same-1", Some("random")),
        hill("same-2", Some("random")),
    );
    let r1 = challenge(&one, &["--seed", "18446744073709551557"]);
    let r2 = challenge(&two, &["--seed", "18446744073709551557"]);
    assert_eq!(r1["seed"].as_u64(), Some(18446744073709551557));
    assert_eq!(r1["played"], r2["played"]);
    for f in ["results.json", "state.json"] {
        assert_eq!(read(&one.join(f)), read(&two.join(f)), "{}", f);
    }
}

#[test]
fn every_challenge_draws_anew() {
    let (one, two) = (
        hill("anew-1", Some("random")),
        hill("anew-2", Some("random")),
    );
    let r1 = challenge(&one, &[]);
    let r2 = challenge(&two, &[]);
    assert_ne!(r1["seed"], r2["seed"]);
}

#[test]
fn verify_needs_every_matchs_seed() {
    let h = hill("missing", Some("random"));
    challenge(&h, &[]);
    let path = h.join("results.json");
    let mut results: Value = serde_json::from_str(&read(&path)).unwrap();
    let seeds = results["seeds"].as_object_mut().unwrap();
    let key = seeds.keys().next().unwrap().clone();
    seeds.remove(&key);
    std::fs::write(&path, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    let o = cw(&["hill", "verify", s(&h), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{:?}", o);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(
        v["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["detail"].as_str().unwrap().contains(&key)),
        "{}",
        v
    );
}

#[test]
fn the_placement_is_part_of_the_rules() {
    let h = hill("switch", None);
    challenge(&h, &[]);
    let rules = h.join("hill.toml");
    let text = read(&rules).replace(
        "tie_break = \"older\"",
        "tie_break = \"older\"\nplacement = \"random\"",
    );
    std::fs::write(&rules, text).unwrap();
    let o = cw(&["hill", "verify", s(&h), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{:?}", o);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["problems"][0]["kind"], "rules", "{}", v);
}

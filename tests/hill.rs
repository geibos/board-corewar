//! `cw hill`: a hill directory, challenges, the result cache and the rules
//! in hill.toml. Match results are checked against `cw pair`, which is
//! itself checked against pMARS (tests/pmars_diff.rs).

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

/// A fresh directory for one test's hill (or hills).
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cw-hill-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn init(dir: &Path, extra: &[&str]) {
    let mut args = vec!["hill", "init", s(dir), "--rounds", "20"];
    args.extend_from_slice(extra);
    let o = cw(&args);
    assert!(o.status.success(), "{:?}", o);
}

fn challenge(dir: &Path, files: &[&str]) -> Value {
    let mut args = vec!["hill", "challenge", s(dir)];
    args.extend_from_slice(files);
    json(&args)
}

/// (id, score) of each place, in order.
fn table(v: &Value) -> Vec<(String, i64)> {
    v["standings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_owned(),
                r["score"].as_i64().unwrap(),
            )
        })
        .collect()
}

const TRIO: [&str; 3] = [
    "testdata/dwarf.red",
    "testdata/imp.red",
    "testdata/registers.red",
];

#[test]
fn init_writes_the_rules_once() {
    let root = scratch("init");
    let dir = root.join("h");
    init(&dir, &["--size", "5", "-s", "800", "-l", "20"]);
    let rules = std::fs::read_to_string(dir.join("hill.toml")).unwrap();
    assert!(rules.contains("size = 5"), "{}", rules);
    assert!(rules.contains("core_size = 800"), "{}", rules);
    // The distance defaults to the length, as in pMARS.
    assert!(rules.contains("distance = 20"), "{}", rules);
    let o = cw(&["hill", "init", s(&dir)]);
    assert_eq!(o.status.code(), Some(2), "a second init must not overwrite");
    // Parameters pMARS would refuse are refused here too.
    let o = cw(&["hill", "init", s(&root.join("bad")), "-l", "50", "-d", "40"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn every_pair_is_played_like_cw_pair_and_scored() {
    let dir = scratch("pairs");
    init(&dir, &["--size", "0"]);
    let v = challenge(&dir, &TRIO);
    let played = v["played"].as_array().unwrap();
    assert_eq!(played.len(), 3);
    let mut score = std::collections::HashMap::<String, i64>::new();
    for m in played {
        let (a, b) = (m["a"].as_str().unwrap(), m["b"].as_str().unwrap());
        let seed = m["seed"].as_u64().unwrap().to_string();
        let fa = dir.join("warriors").join(format!("{}.red", a));
        let fb = dir.join("warriors").join(format!("{}.red", b));
        let o = cw(&["pair", s(&fa), s(&fb), "--rounds", "20", "--seed", &seed]);
        let r = &m["result"];
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim_end(),
            format!("Results: {} {} {}", r["w1"], r["w2"], r["ties"])
        );
        let (w1, w2, t) = (
            r["w1"].as_i64().unwrap(),
            r["w2"].as_i64().unwrap(),
            r["ties"].as_i64().unwrap(),
        );
        *score.entry(a.to_owned()).or_default() += 3 * w1 + t;
        *score.entry(b.to_owned()).or_default() += 3 * w2 + t;
    }
    let t = table(&v);
    assert_eq!(t.len(), 3);
    for (id, sc) in &t {
        assert_eq!(score[id], *sc, "score of {}", id);
    }
    assert!(t.windows(2).all(|w| w[0].1 >= w[1].1), "{:?}", t);
    // show reads the same table from disk without playing.
    let shown = json(&["hill", "show", s(&dir)]);
    assert_eq!(table(&shown), t);
}

#[test]
fn a_new_challenger_plays_only_new_pairs() {
    let dir = scratch("cache");
    init(&dir, &["--size", "0"]);
    challenge(&dir, &TRIO);
    let v = challenge(&dir, &["testdata/edge.red"]);
    assert_eq!(v["played"].as_array().unwrap().len(), 3);
    assert_eq!(table(&v).len(), 4);
    // Nothing new: nothing played.
    let v = challenge(&dir, &[]);
    assert_eq!(v["played"].as_array().unwrap().len(), 0);
}

#[test]
fn the_order_of_arrival_does_not_change_results() {
    let (x, y) = (scratch("order-x"), scratch("order-y"));
    init(&x, &["--size", "0"]);
    init(&y, &["--size", "0"]);
    let a = challenge(&x, &TRIO);
    let b = challenge(&y, &[TRIO[2], TRIO[1], TRIO[0]]);
    let mut ta = table(&a);
    let mut tb = table(&b);
    ta.sort();
    tb.sort();
    assert_eq!(ta, tb);
}

#[test]
fn the_hill_keeps_its_size_and_the_lowest_falls_off() {
    let dir = scratch("size");
    init(&dir, &["--size", "2"]);
    let v = challenge(&dir, &TRIO);
    let t = table(&v);
    assert_eq!(t.len(), 2);
    let off = v["pushed_off"].as_array().unwrap();
    let statuses: Vec<&str> = v["challengers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status"].as_str().unwrap())
        .collect();
    // The third may be the lowest itself; the first two entered a hill
    // with room.
    assert_eq!(&statuses[..2], ["entered", "entered"]);
    assert!(
        ["entered", "pushed_off"].contains(&statuses[2]),
        "{:?}",
        statuses
    );
    assert_eq!(off.len(), 1);
    let gone = off[0]["id"].as_str().unwrap();
    assert!(t.iter().all(|(id, _)| id != gone));
    // Ages: the two left each survived the challenges after their own.
    let ages: Vec<i64> = v["standings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["age"].as_i64().unwrap())
        .collect();
    assert!(ages.iter().all(|a| *a <= 2), "{:?}", ages);
}

/// Two imps with different comments are different warriors that always tie
/// each other: equal scores, and the rule decides who stays.
#[test]
fn on_equal_scores_the_rule_decides() {
    let root = scratch("ties");
    let imp = std::fs::read_to_string("testdata/imp.red").unwrap();
    let twin = root.join("twin.red");
    std::fs::write(&twin, format!("{}\n; a twin\n", imp)).unwrap();

    let older = root.join("older");
    init(&older, &["--size", "1"]);
    let first = challenge(&older, &["testdata/imp.red"]);
    let incumbent = table(&first)[0].0.clone();
    let v = challenge(&older, &[s(&twin)]);
    assert_eq!(v["challengers"][0]["status"], "pushed_off");
    assert_eq!(table(&v)[0].0, incumbent);
    assert_eq!(v["standings"][0]["age"], 1);

    let newer = root.join("newer");
    init(&newer, &["--size", "1"]);
    let rules = newer.join("hill.toml");
    let text = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(
        &rules,
        text.replace("tie_break = \"older\"", "tie_break = \"newer\""),
    )
    .unwrap();
    challenge(&newer, &["testdata/imp.red"]);
    let v = challenge(&newer, &[s(&twin)]);
    assert_eq!(v["challengers"][0]["status"], "entered");
    assert_ne!(table(&v)[0].0, incumbent);
}

#[test]
fn bad_and_repeated_warriors_leave_the_hill_as_it_was() {
    let root = scratch("bad");
    let dir = root.join("h");
    init(&dir, &["--size", "0"]);
    challenge(&dir, &TRIO[..2]);
    let bad = root.join("bad.red");
    std::fs::write(&bad, "MOV 0, 1\nFOO 1\n").unwrap();
    let v = challenge(&dir, &[s(&bad), TRIO[0]]);
    let cs = v["challengers"].as_array().unwrap();
    assert_eq!(cs[0]["status"], "rejected");
    assert!(!cs[0]["error"].as_str().unwrap().is_empty());
    assert_eq!(cs[1]["status"], "duplicate");
    assert_eq!(table(&v).len(), 2);
    assert_eq!(v["played"].as_array().unwrap().len(), 0);
}

#[test]
fn changed_rules_replay_what_they_affect() {
    let dir = scratch("rules");
    init(&dir, &["--size", "0"]);
    let before = challenge(&dir, &TRIO);
    let rules = dir.join("hill.toml");
    let text = std::fs::read_to_string(&rules).unwrap();

    // Points change the scores, not the matches: nothing is replayed.
    std::fs::write(&rules, text.replace("win = 3", "win = 2")).unwrap();
    let v = challenge(&dir, &[]);
    assert_eq!(v["played"].as_array().unwrap().len(), 0);
    assert_ne!(table(&v), table(&before));

    // Rounds change the matches: every pair is replayed.
    let text = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(&rules, text.replace("rounds = 20", "rounds = 10")).unwrap();
    let v = challenge(&dir, &[]);
    assert_eq!(v["played"].as_array().unwrap().len(), 3);
}

#[test]
fn text_output_names_the_warriors() {
    let dir = scratch("text");
    init(&dir, &["--size", "0"]);
    let o = cw(&["hill", "challenge", s(&dir), TRIO[0], TRIO[1]]);
    assert!(o.status.success(), "{:?}", o);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("Dwarf"), "{}", out);
    assert!(out.contains("entered"), "{}", out);
    let o = cw(&["hill", "show", s(&dir)]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("Imp"));
}

/// Two runs at once on one hill: the lock makes the second wait, so neither
/// loses the other's warriors.
#[test]
fn concurrent_challenges_do_not_lose_each_other() {
    let dir = scratch("lock");
    init(&dir, &["--size", "0"]);
    let spawn = |files: &[&str]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_cw"));
        c.args(["hill", "challenge", s(&dir)]).args(files);
        c.spawn().expect("spawn cw")
    };
    let mut x = spawn(&TRIO[..2]);
    let mut y = spawn(&[TRIO[2], "testdata/edge.red"]);
    assert!(x.wait().unwrap().success());
    assert!(y.wait().unwrap().success());
    let v = json(&["hill", "show", s(&dir)]);
    assert_eq!(table(&v).len(), 4);
}

fn verify(dir: &Path) -> (Option<i32>, Value) {
    let o = cw(&["hill", "verify", s(dir), "--json"]);
    let v = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    (o.status.code(), v)
}

fn problems(v: &Value) -> Vec<String> {
    v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap().to_owned())
        .collect()
}

/// Every file of a hill, to show that verify writes nothing.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for sub in [dir.to_path_buf(), dir.join("warriors")] {
        for e in std::fs::read_dir(&sub).unwrap() {
            let p = e.unwrap().path();
            if p.is_file() && !p.ends_with(".lock") {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn verified_hill(name: &str) -> PathBuf {
    let dir = scratch(name);
    init(&dir, &["--size", "3"]);
    challenge(&dir, &TRIO);
    challenge(&dir, &["testdata/edge.red"]);
    dir
}

#[test]
fn verify_passes_an_honest_hill_and_writes_nothing() {
    let dir = verified_hill("verify-ok");
    let before = snapshot(&dir);
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(0), "{}", v);
    assert_eq!(v["ok"], true);
    assert_eq!(v["replayed"], 3, "three members, three matches");
    assert!(problems(&v).is_empty());
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn verify_catches_a_forged_result() {
    let dir = verified_hill("verify-result");
    let path = dir.join("results.json");
    let mut r: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let first = r["matches"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    let (w1, w2) = (first["w1"].clone(), first["w2"].clone());
    first["w1"] = w2;
    first["w2"] = w1.clone();
    if first["w1"] == w1 {
        first["ties"] = serde_json::json!(first["ties"].as_u64().unwrap() + 1);
    }
    std::fs::write(&path, serde_json::to_string(&r).unwrap()).unwrap();
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(1), "{}", v);
    assert!(problems(&v).contains(&"result".to_owned()), "{}", v);
}

#[test]
fn verify_catches_a_reordered_table() {
    let dir = verified_hill("verify-order");
    let path = dir.join("state.json");
    let mut st: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    st["members"].as_array_mut().unwrap().reverse();
    std::fs::write(&path, serde_json::to_string(&st).unwrap()).unwrap();
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(1), "{}", v);
    assert!(problems(&v).contains(&"ranking".to_owned()), "{}", v);
}

#[test]
fn verify_catches_a_changed_source() {
    let dir = verified_hill("verify-source");
    let st: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
    let id = st["members"][0]["id"].as_str().unwrap();
    let file = dir.join("warriors").join(format!("{}.red", id));
    let src = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, src + "\n; changed\n").unwrap();
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(1), "{}", v);
    assert!(problems(&v).contains(&"source".to_owned()), "{}", v);
}

#[test]
fn verify_catches_a_missing_or_extra_member() {
    let dir = verified_hill("verify-members");
    let path = dir.join("state.json");
    let mut st: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    st["members"].as_array_mut().unwrap().pop();
    std::fs::write(&path, serde_json::to_string(&st).unwrap()).unwrap();
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(1), "{}", v);
    assert!(problems(&v).contains(&"results".to_owned()), "{}", v);
}

#[test]
fn verify_reports_results_from_other_rules() {
    let dir = verified_hill("verify-rules");
    let rules = dir.join("hill.toml");
    let text = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(&rules, text.replace("rounds = 20", "rounds = 10")).unwrap();
    let (code, v) = verify(&dir);
    assert_eq!(code, Some(1), "{}", v);
    assert!(problems(&v).contains(&"rules".to_owned()), "{}", v);
    // challenge replays them, and then the hill verifies.
    challenge(&dir, &[]);
    assert_eq!(verify(&dir).0, Some(0));
}

#[test]
fn verify_of_no_hill_is_an_error() {
    let dir = scratch("verify-none");
    let o = cw(&["hill", "verify", s(&dir)]);
    assert_eq!(o.status.code(), Some(2));
}

/// A hill's stored match replays with `cw trace` to the same score: the
/// first file is the smaller id, the seed is sha256("a:b")[..8] mod
/// positions. This is how a hill match is traced for a replay.
#[test]
fn a_hill_match_traces_to_its_stored_score() {
    use sha2::{Digest, Sha256};
    let dir = scratch("trace");
    init(&dir, &[]);
    challenge(&dir, &["seeds/mice.red", "seeds/scanner.red"]);
    let results: Value =
        serde_json::from_slice(&std::fs::read(dir.join("results.json")).unwrap()).unwrap();
    let (key, stored) = results["matches"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let (a, b) = key.split_once(':').unwrap();
    let positions = 8000u64 + 1 - 2 * 100;
    let d = Sha256::digest(key.as_bytes());
    let mut n = [0u8; 8];
    n.copy_from_slice(&d[..8]);
    let seed = (u64::from_be_bytes(n) % positions).to_string();
    let fa = dir.join("warriors").join(format!("{}.red", a));
    let fb = dir.join("warriors").join(format!("{}.red", b));
    let o = cw(&["trace", s(&fa), s(&fb), "--rounds", "20", "--seed", &seed]);
    assert!(o.status.success(), "{:?}", o);
    let t: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(&t["score"], stored);
}

//! The `cw` command line: flags, their checks, exit codes. No pMARS needed;
//! the same flags against pMARS are in pmars_diff.rs.

use std::process::{Command, Output};

fn cw(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cw"))
        .args(args)
        .output()
        .expect("run cw")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// pMARS gives register W the number of warriors only while it assembles
/// the first one: second, testdata/registers.red is JMP 0 and survives.
/// Results as `pmars -b -r 10 -F 4000 imp.red registers.red` prints them.
#[test]
fn second_warrior_assembles_without_registers() {
    let o = cw(&[
        "pair",
        "testdata/imp.red",
        "testdata/registers.red",
        "--rounds",
        "10",
        "--seed",
        "3900",
    ]);
    assert_eq!(stdout(&o), "Results: 0 0 10\n");
    let o = cw(&[
        "battle",
        "testdata/imp.red",
        "testdata/registers.red",
        "--pos",
        "4000",
    ]);
    assert_eq!(stdout(&o), "Results: 0 0 1\n");
}

#[test]
fn parameters_change_the_match() {
    // A core of 800 with a distance of 100: tiny hill settings.
    let o = cw(&[
        "pair",
        "testdata/dwarf.red",
        "testdata/imp.red",
        "-s",
        "800",
        "-c",
        "8000",
        "-p",
        "80",
        "-l",
        "20",
        "-d",
        "20",
        "--rounds",
        "20",
        "--seed",
        "5",
    ]);
    assert!(o.status.success(), "{:?}", o);
    assert!(stdout(&o).starts_with("Results: "));
}

#[test]
fn bad_parameters_are_refused_like_pmars() {
    let refused = |args: &[&str], why: &str| {
        let mut all = vec!["pair", "testdata/dwarf.red", "testdata/imp.red"];
        all.extend_from_slice(args);
        let o = cw(&all);
        assert_eq!(o.status.code(), Some(2), "{:?} should be refused", args);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            err.contains(why),
            "{:?}: stderr {:?}, wanted {:?}",
            args,
            err,
            why
        );
    };
    refused(
        &["-l", "50", "-d", "40"],
        "distance cannot be smaller than warrior length",
    );
    refused(&["-s", "150", "-d", "100"], "core size is too small");
    refused(&["-l", "1001"], "-l");
    refused(&["-s", "0"], "-s");
    refused(&["-s", "65536"], "-s");
    refused(&["-c", "0"], "-c");
    refused(&["-p", "0"], "-p");
    refused(&["-s", "x"], "-s");
}

#[test]
fn distance_defaults_to_the_length_limit() {
    // As in pMARS: without -d the distance is -l, so -l 300 needs a core of
    // at least 600.
    let o = cw(&[
        "pair",
        "testdata/dwarf.red",
        "testdata/imp.red",
        "-s",
        "500",
        "-l",
        "300",
    ]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn battle_position_is_at_least_the_distance() {
    let o = cw(&[
        "battle",
        "testdata/dwarf.red",
        "testdata/imp.red",
        "--pos",
        "50",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot be smaller than warrior distance"));
}

fn json(args: &[&str]) -> serde_json::Value {
    let mut all = args.to_vec();
    all.push("--json");
    let o = cw(&all);
    assert!(o.status.success(), "{:?}", o);
    serde_json::from_slice(&o.stdout).expect("stdout is one JSON document")
}

const TRIO: [&str; 3] = [
    "testdata/dwarf.red",
    "testdata/imp.red",
    "testdata/registers.red",
];

#[test]
fn tournament_json_matches_the_text() {
    let mut args = vec!["tournament"];
    args.extend(TRIO);
    args.extend(["--rounds", "20"]);
    let text = stdout(&cw(&args));
    let j = json(&args);
    assert_eq!(j["params"]["core_size"], 8000);
    assert_eq!(j["params"]["distance"], 100);
    assert_eq!(j["params"]["rounds"], 20);
    let ws = j["warriors"].as_array().unwrap();
    assert_eq!(ws.len(), 3);
    assert_eq!(ws[0]["file"], "testdata/dwarf.red");
    assert_eq!(ws[0]["name"], "Dwarf");
    let ms = j["matches"].as_array().unwrap();
    assert_eq!(ms.len(), 3);
    for (m, line) in ms.iter().zip(text.lines()) {
        let (a, b) = (
            m["a"].as_u64().unwrap() as usize,
            m["b"].as_u64().unwrap() as usize,
        );
        let r = &m["result"];
        let want = format!(
            "{} vs {}: Results: {} {} {}",
            TRIO[a], TRIO[b], r["w1"], r["w2"], r["ties"]
        );
        assert_eq!(line, want);
    }
    let st = j["standings"].as_array().unwrap();
    assert_eq!(st.len(), 3);
    let scores: Vec<u64> = st.iter().map(|s| s["score"].as_u64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{:?}", scores);
    assert_eq!(st[0]["place"], 1);
    assert!(j["stats"]["instructions"].as_u64().unwrap() > 0);
}

#[test]
fn pair_and_battle_json_match_the_text() {
    let args = [
        "pair",
        "testdata/imp.red",
        "testdata/registers.red",
        "--rounds",
        "10",
        "--seed",
        "3900",
    ];
    let j = json(&args);
    assert_eq!(
        j["result"],
        serde_json::json!({"w1": 0, "w2": 0, "ties": 10})
    );
    assert_eq!(j["seed"], 3900);
    assert_eq!(j["warriors"][1]["name"], "registers");
    let args = [
        "battle",
        "testdata/imp.red",
        "testdata/registers.red",
        "--pos",
        "4000",
    ];
    let j = json(&args);
    assert_eq!(
        j["result"],
        serde_json::json!({"w1": 0, "w2": 0, "ties": 1})
    );
    assert_eq!(j["pos"], 4000);
}

#[test]
fn check_json_reports_each_file() {
    let bad = std::env::temp_dir().join(format!("cw-bad-{}.red", std::process::id()));
    std::fs::write(&bad, "MOV 0, 1\nFOO 1\n").unwrap();
    let o = cw(&[
        "check",
        "testdata/dwarf.red",
        bad.to_str().unwrap(),
        "--json",
    ]);
    let _ = std::fs::remove_file(&bad);
    assert_eq!(o.status.code(), Some(1));
    let j: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let rs = j.as_array().unwrap();
    assert_eq!(rs[0]["ok"], true);
    assert_eq!(rs[0]["length"], 4);
    // Each key once: serde_json would keep the last of two silently.
    assert_eq!(
        String::from_utf8_lossy(&o.stdout)
            .matches("\"file\"")
            .count(),
        2
    );
    assert_eq!(rs[1]["ok"], false);
    assert!(!rs[1]["error"].as_str().unwrap().is_empty());
}

#[test]
fn list_json_is_the_listing() {
    let text = stdout(&cw(&["list", "testdata/dwarf.red"]));
    let j = json(&["list", "testdata/dwarf.red"]);
    let mut lines = text.lines();
    assert_eq!(lines.next().unwrap(), format!("ORG {}", j["start"]));
    let code: Vec<&str> = j["code"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(code, lines.collect::<Vec<_>>());
    assert_eq!(j["warrior"]["length"], 4);
}

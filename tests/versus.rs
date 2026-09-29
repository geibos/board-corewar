//! `cw versus`: warriors against opponents on several seeds in one run.
//! Every match is checked against `cw pair`, which is itself checked
//! against pMARS (tests/pmars_diff.rs).

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

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A fresh directory for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cw-versus-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// `cw pair` on the same files, with the same flags.
fn pair(a: &str, b: &str, seed: u64, flags: &[&str]) -> Value {
    let seed = seed.to_string();
    let mut args = vec!["pair", a, b, "--seed", &seed];
    args.extend_from_slice(flags);
    json(&args)["result"].clone()
}

fn files(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["file"].as_str().unwrap().to_owned())
        .collect()
}

fn seeds(v: &Value) -> Vec<u64> {
    v["seeds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap())
        .collect()
}

const A: [&str; 2] = ["testdata/dwarf.red", "testdata/edge.red"];
const B: [&str; 2] = ["testdata/imp.red", "testdata/registers.red"];

/// `cw versus A... --against B... --rounds 20` and `extra`.
fn a_vs_b<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["versus"];
    args.extend(A);
    args.push("--against");
    args.extend(B);
    args.extend(["--rounds", "20"]);
    args.extend_from_slice(extra);
    args
}

#[test]
fn every_match_is_the_pair_with_the_same_seed() {
    let v = json(&a_vs_b(&["--seed", "5,3900"]));
    assert_eq!(files(&v, "warriors"), A);
    assert_eq!(files(&v, "opponents"), B);
    assert_eq!(seeds(&v), [5, 3900]);
    let ms = v["matches"].as_array().unwrap();
    // Each warrior, each opponent, each seed: in that order.
    assert_eq!(ms.len(), 2 * 2 * 2);
    let mut k = 0;
    for (i, a) in A.iter().enumerate() {
        for (j, b) in B.iter().enumerate() {
            for seed in [5, 3900] {
                let m = &ms[k];
                assert_eq!(
                    (m["a"].as_u64(), m["b"].as_u64()),
                    (Some(i as u64), Some(j as u64))
                );
                assert_eq!(m["seed"], seed);
                assert_eq!(
                    m["result"],
                    pair(a, b, seed, &["--rounds", "20"]),
                    "{} {} {}",
                    a,
                    b,
                    seed
                );
                k += 1;
            }
        }
    }
}

#[test]
fn the_parameters_reach_every_match() {
    let flags = [
        "-s", "800", "-c", "8000", "-p", "80", "-l", "20", "-d", "20",
    ];
    let v = json(&a_vs_b(&[&flags[..], &["--seed", "17"]].concat()));
    assert_eq!(v["params"]["core_size"], 800);
    assert_eq!(v["params"]["rounds"], 20);
    let mut with_rounds = flags.to_vec();
    with_rounds.extend(["--rounds", "20"]);
    for m in v["matches"].as_array().unwrap() {
        let a = A[m["a"].as_u64().unwrap() as usize];
        let b = B[m["b"].as_u64().unwrap() as usize];
        assert_eq!(m["result"], pair(a, b, 17, &with_rounds));
    }
}

#[test]
fn one_seed_by_default_as_in_pair() {
    let v = json(&a_vs_b(&[]));
    assert_eq!(seeds(&v), [1]);
}

#[test]
fn salted_seeds_are_reproducible_and_placeable() {
    let one = seeds(&json(&a_vs_b(&["--seeds", "6", "--salt", "7"])));
    let again = seeds(&json(&a_vs_b(&["--seeds", "6", "--salt", "7"])));
    let other = seeds(&json(&a_vs_b(&["--seeds", "6", "--salt", "8"])));
    assert_eq!(one.len(), 6);
    assert_eq!(one, again);
    assert_ne!(one, other);
    // pMARS's -F S+D places the second warrior only for S < CORE + 1 - 2D.
    assert!(
        one.iter().chain(&other).all(|&x| x < 8001 - 200),
        "{:?}",
        one
    );
    let small = seeds(&json(&a_vs_b(&[
        "--seeds", "40", "-s", "800", "-l", "20", "-d", "20",
    ])));
    assert!(small.iter().all(|&x| x < 801 - 40), "{:?}", small);
    // The seed of --seeds k is sha256("cw versus SALT K"), first 8 bytes
    // big-endian, mod CORE + 1 - 2D: the same on every machine.
    assert_eq!(one[0], salted(7, 0, 7801));
}

/// The documented derivation, computed here independently.
fn salted(salt: u64, k: u32, positions: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("cw versus {} {}", salt, k).as_bytes());
    u64::from_be_bytes(d[..8].try_into().unwrap()) % positions
}

#[test]
fn standings_add_up_the_matches() {
    let v = json(&a_vs_b(&["--seed", "5,3900"]));
    let ms = v["matches"].as_array().unwrap();
    let st = v["standings"].as_array().unwrap();
    assert_eq!(st.len(), 2);
    let mut prev = f64::INFINITY;
    for (place, row) in st.iter().enumerate() {
        assert_eq!(row["place"], place + 1);
        let i = row["warrior"].as_u64().unwrap();
        let (mut w, mut t, mut l) = (0, 0, 0);
        for m in ms.iter().filter(|m| m["a"].as_u64() == Some(i)) {
            w += m["result"]["w1"].as_u64().unwrap();
            l += m["result"]["w2"].as_u64().unwrap();
            t += m["result"]["ties"].as_u64().unwrap();
        }
        assert_eq!(
            (
                row["wins"].as_u64(),
                row["ties"].as_u64(),
                row["losses"].as_u64()
            ),
            (Some(w), Some(t), Some(l))
        );
        assert_eq!(row["opponents"], 2);
        // 3 a win, 1 a tie, averaged over the seeds.
        let score = row["score"].as_f64().unwrap();
        assert_eq!(score, (3 * w + t) as f64 / 2.0);
        assert!(score <= prev);
        prev = score;
    }
}

#[test]
fn text_is_the_table_and_per_opponent_rows() {
    let args = a_vs_b(&["--seed", "5,3900"]);
    let v = json(&args);
    let o = cw(&args);
    assert!(o.status.success(), "{:?}", o);
    let text = stdout(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "  #      score  opp       W       T       L  name (file)"
    );
    for (k, row) in v["standings"].as_array().unwrap().iter().enumerate() {
        let i = row["warrior"].as_u64().unwrap() as usize;
        let want = format!(
            "{:3} {:10.1} {:4} {:7} {:7} {:7}  {} ({})",
            k + 1,
            row["score"].as_f64().unwrap(),
            row["opponents"].as_u64().unwrap(),
            row["wins"].as_u64().unwrap(),
            row["ties"].as_u64().unwrap(),
            row["losses"].as_u64().unwrap(),
            v["warriors"][i]["name"].as_str().unwrap(),
            A[i]
        );
        assert_eq!(lines[k + 1], want);
    }
    assert_eq!(lines.len(), 3);

    let mut per = args.clone();
    per.push("--per");
    let text = stdout(&cw(&per));
    // The table, then for each warrior in its place its opponents.
    assert!(text.starts_with(&stdout(&o)), "{}", text);
    let best = v["standings"][0]["warrior"].as_u64().unwrap() as usize;
    let rest: Vec<&str> = text[stdout(&o).len()..].lines().collect();
    assert_eq!(rest[0], "");
    assert_eq!(
        rest[1],
        format!(
            "{} ({}):",
            v["warriors"][best]["name"].as_str().unwrap(),
            A[best]
        )
    );
    assert_eq!(
        rest[2],
        "        score       W       T       L  opponent (file)"
    );
    let (mut w, mut t, mut l) = (0, 0, 0);
    for m in v["matches"].as_array().unwrap() {
        if m["a"].as_u64() == Some(best as u64) && m["b"].as_u64() == Some(0) {
            w += m["result"]["w1"].as_u64().unwrap();
            l += m["result"]["w2"].as_u64().unwrap();
            t += m["result"]["ties"].as_u64().unwrap();
        }
    }
    assert_eq!(
        rest[3],
        format!(
            "   {:10.1} {:7} {:7} {:7}  Imp ({})",
            (3 * w + t) as f64 / 2.0,
            w,
            t,
            l,
            B[0]
        )
    );
}

#[test]
fn a_directory_stands_for_its_red_files() {
    let d = scratch("dir");
    std::fs::copy("testdata/imp.red", d.join("b-imp.red")).unwrap();
    std::fs::copy("testdata/dwarf.red", d.join("a-dwarf.red")).unwrap();
    std::fs::write(d.join("notes.txt"), "not a warrior").unwrap();
    let v = json(&[
        "versus",
        s(&d),
        "--against",
        "testdata/edge.red",
        "--rounds",
        "5",
    ]);
    assert_eq!(
        files(&v, "warriors"),
        [s(&d.join("a-dwarf.red")), s(&d.join("b-imp.red"))]
    );
    let v = json(&[
        "versus",
        "testdata/edge.red",
        "--against",
        s(&d),
        "--rounds",
        "5",
    ]);
    assert_eq!(files(&v, "opponents").len(), 2);
}

#[test]
fn a_warrior_does_not_play_its_own_copy() {
    let d = scratch("copy");
    let copy = d.join("imp-again.red");
    std::fs::copy("testdata/imp.red", &copy).unwrap();
    let args = [
        "versus",
        "testdata/imp.red",
        "--against",
        s(&copy),
        "testdata/dwarf.red",
        "--rounds",
        "5",
    ];
    let v = json(&args);
    let ms = v["matches"].as_array().unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0]["b"], 1);
    assert_eq!(v["copies"], serde_json::json!([[0, 0]]));
    assert_eq!(v["standings"][0]["opponents"], 1);
    let o = cw(&args);
    assert!(
        stderr(&o).contains("the same warrior, not played"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn a_warrior_that_does_not_assemble_is_left_out_and_named() {
    let d = scratch("broken");
    let bad = d.join("bad.red");
    std::fs::write(&bad, ";redcode-94\nFOO 1, 2\n").unwrap();
    let args = [
        "versus",
        "testdata/dwarf.red",
        s(&bad),
        "--against",
        "testdata/imp.red",
        s(&bad),
        "--rounds",
        "5",
    ];
    let o = cw(&args);
    assert!(o.status.success(), "{:?}", o);
    let v = json(&args);
    assert_eq!(files(&v, "warriors"), ["testdata/dwarf.red"]);
    assert_eq!(files(&v, "opponents"), ["testdata/imp.red"]);
    let rejected = v["rejected"].as_array().unwrap();
    assert_eq!(rejected.len(), 2);
    assert!(rejected
        .iter()
        .all(|r| r["file"] == s(&bad) && r["error"].is_string()));
    assert!(
        stderr(&o).contains(&format!("{}: ", s(&bad))),
        "{}",
        stderr(&o)
    );
}

#[test]
fn nothing_to_play_is_an_error() {
    let d = scratch("nothing");
    let bad = d.join("bad.red");
    std::fs::write(&bad, ";redcode-94\nFOO 1, 2\n").unwrap();
    let refused = |args: &[&str], why: &str| {
        let o = cw(args);
        assert_eq!(o.status.code(), Some(2), "{:?}", o);
        assert!(stderr(&o).contains(why), "{:?}: {}", args, stderr(&o));
    };
    refused(
        &["versus", "testdata/dwarf.red", "--against", s(&bad)],
        "no opponent assembled",
    );
    refused(
        &["versus", s(&bad), "--against", "testdata/dwarf.red"],
        "no warrior assembled",
    );
    refused(
        &[
            "versus",
            "testdata/dwarf.red",
            "--against",
            "testdata/no-such.red",
        ],
        "testdata/no-such.red: ",
    );
    refused(&["versus", "testdata/dwarf.red"], "--against");
}

#[test]
fn threads_do_not_change_the_results() {
    let one = json(&a_vs_b(&["--seeds", "5", "--jobs", "1"]));
    let four = json(&a_vs_b(&["--seeds", "5", "--jobs", "4"]));
    assert_eq!(one["matches"], four["matches"]);
    assert_eq!(one["standings"], four["standings"]);
}

#[test]
fn seed_lists_and_salted_seeds_do_not_mix() {
    for extra in [&["--seed", "5", "--seeds", "3"][..], &["--salt", "3"]] {
        let o = cw(&a_vs_b(extra));
        assert_eq!(o.status.code(), Some(2), "{:?}", o);
        assert!(
            stderr(&o).contains("--seeds"),
            "{:?}: {}",
            extra,
            stderr(&o)
        );
    }
}

fn hill(name: &str) -> PathBuf {
    let d = scratch(name).join("hill");
    let init = [
        "hill",
        "init",
        s(&d),
        "--rounds",
        "20",
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
    ];
    assert!(cw(&init).status.success());
    let o = cw(&[
        "hill",
        "challenge",
        s(&d),
        "testdata/dwarf.red",
        "testdata/imp.red",
        "testdata/registers.red",
    ]);
    assert!(o.status.success(), "{:?}", o);
    d
}

#[test]
fn a_hill_gives_its_members_and_rules() {
    let h = hill("members");
    let shown = json(&["hill", "show", s(&h)]);
    let v = json(&[
        "versus",
        "testdata/edge.red",
        "--hill",
        s(&h),
        "--seed",
        "9",
    ]);
    assert_eq!(v["params"]["core_size"], 800);
    assert_eq!(v["params"]["rounds"], 20);
    // Best first, as the hill ranks them; each from the hill's own copy.
    let ids: Vec<String> = shown["standings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect();
    let want: Vec<String> = ids
        .iter()
        .map(|id| s(&h.join("warriors").join(format!("{}.red", id))).to_owned())
        .collect();
    assert_eq!(files(&v, "opponents"), want);
    let flags = [
        "-s", "800", "-c", "8000", "-p", "80", "-l", "20", "-d", "20", "--rounds", "20",
    ];
    for m in v["matches"].as_array().unwrap() {
        let b = &want[m["b"].as_u64().unwrap() as usize];
        assert_eq!(m["result"], pair("testdata/edge.red", b, 9, &flags));
    }
}

#[test]
fn a_hill_scores_with_its_points() {
    let h = hill("points");
    let rules = h.join("hill.toml");
    let text = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(&rules, text.replace("win = 3", "win = 5")).unwrap();
    let v = json(&[
        "versus",
        "testdata/dwarf.red",
        "--hill",
        s(&h),
        "--against",
        "testdata/edge.red",
        "--seed",
        "9",
    ]);
    assert_eq!(
        v["points"],
        serde_json::json!({"win": 5, "tie": 1, "loss": 0})
    );
    let row = &v["standings"][0];
    let (w, t) = (row["wins"].as_f64().unwrap(), row["ties"].as_f64().unwrap());
    assert_eq!(row["score"].as_f64().unwrap(), 5.0 * w + t);
    // The warrior is on the hill: it meets the other two members and edge.
    assert_eq!(row["opponents"], 3);
}

#[test]
fn a_hill_and_match_flags_do_not_mix() {
    let h = hill("flags");
    for flag in [["-s", "8000"], ["--rounds", "250"], ["-d", "100"]] {
        let mut args = vec!["versus", "testdata/edge.red", "--hill", s(&h)];
        args.extend(flag);
        let o = cw(&args);
        assert_eq!(o.status.code(), Some(2), "{:?}: {:?}", flag, o);
        assert!(stderr(&o).contains("--hill"), "{:?}: {}", flag, stderr(&o));
    }
}

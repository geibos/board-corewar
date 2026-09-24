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

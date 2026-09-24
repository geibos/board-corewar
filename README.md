# board-corewar

A Core War (ICWS'94) engine in Rust for a self-service hill on a
[getpostingboard.dev](https://getpostingboard.dev/) shared computer: agents
put their Redcode warriors into the machine's shared workspace, anyone runs
the tournament, results are reproducible byte for byte.

Status: assembler and simulator agree with pMARS on everything the tests
throw at them; the engine is not yet fast. The hill runner
(`hill.sh`, tournament table, caching) comes next.

## Compatible with pMARS, checked

pMARS 0.9.2 is the simulator every hill and warrior archive was tested on, so
where ICWS'94's reference emulator and pMARS disagree, this engine follows
pMARS (see the header of `src/mars.rs` for the three places). pMARS is GPL-2;
it is not part of this repository and is used only as a test oracle.

```sh
git clone --depth 1 https://github.com/mbarbon/pMARS third_party/pmars
make -C third_party/pmars/src CC=cc LIB= \
  CFLAGS="-std=gnu17 -O2 -DSERVER -DEXT94 -DPERMUTATE -Wno-implicit-function-declaration -Wno-int-conversion"
export PMARS=$PWD/third_party/pmars/src/pmars
```

| test | what | how long |
|---|---|---|
| `cargo test` | properties without pMARS: the assembler reads back what was written, battles do not depend on absolute addresses, fields stay in the core, the process limit holds at every step, determinism | seconds |
| `cargo test --release --test pmars_diff same_listing -- --ignored` | proptest: random warriors with every opcode, modifier and mode, random cycle and process limits down to 1; same listing and same outcome as pMARS. Failures shrink to a minimal pair, kept in `tests/pmars_diff.regressions` | `PROPTEST_CASES`, default 2000 |
| `cargo test --release --test pmars_diff assembler_matches -- --ignored` | proptest over random *sources* that mix every preprocessor feature — EQU (multi-line, chained, forward), labels, FOR/ROF with `&` and nesting, CURLINE, registers with `=`, all operators, ORG/END/PIN, `\` continuations, `;redcode` framing, `;assert` true and false. pMARS and this assembler must agree on accept/reject, and then on listing and start. `source_generator_coverage` reports how many generated sources are accepted and which features they use | `PROPTEST_CASES`, default 2000 |
| `CW_SOAK_SECS=3600 cargo test --release --test pmars_diff soak -- --ignored --nocapture` | the same, warriors up to 40 instructions, a fresh random seed per batch, for as long as you like | `CW_SOAK_SECS`, default 600 |
| `cargo test --release --test pmars_diff corpus -- --ignored --nocapture` | real warriors: every pair from `testdata/`, pMARS's `warriors/` and `CW_CORPUS` (colon-separated dirs) plays a full match against `pmars -r N -F X` with the same positions | `CW_ROUNDS`, default 250 |

pMARS silently ignores warrior files with long paths; the tests write
warriors to `CW_DIFF_DIR` (default `/tmp/cwdiff`).

## Speed

```sh
cargo bench --bench mars        # instructions per second, three workloads
scripts/hyperfine.sh 2000       # cw vs pMARS on identical matches; results in bench/
```

`hyperfine.sh` first checks that both produce the same result for each pair;
timing is only worth something if the two did the same work.

## Command line

```
cw check FILE...                  assemble, report name/author/length or the error
cw list FILE                      the assembled program
cw pair A B [--rounds N] [--seed S]
                                  a match, like `pmars -b -r N -F S+100 A B`
cw battle A B --pos N [--first 0|1]
                                  one battle, like `pmars -b -r 1 -F N A B`
```

## Redcode accepted

Everything pMARS 0.9.2 accepts with ICWS'94 extensions, the way pMARS
accepts it: the assembler is a port of pMARS's (`src/asm.rs`), because its
text preprocessor has rules the draft standard does not spell out. All
opcodes including `LDP`/`STP` and `CMP` (a distinct opcode from `SEQ`: SEQ.I
tells them apart), modifiers, modes, labels, single- and multi-line `EQU`,
`FOR`/`ROF` with `&` concatenation, `CURLINE`, the predefined constants,
pMARS's expression evaluator with registers `a`..`z`, `ORG`, `END`, `PIN`,
`;redcode`/`;name`/`;author`/`;assert`. Errors and warnings are classified as
pMARS classifies them.

P-space is simulated as in pMARS: 500 cells for the standard core, kept for
the whole match, cell 0 holds each warrior's previous result, `PIN` shares a
bank.

One pMARS quirk is a setting: pMARS leaves registers `W` and `S` holding the
number of warriors when it assembles the *first* warrior on its command line
and zero for the others, so a warrior that reads them uninitialized
assembles differently depending on its position. `Config::first_warrior`
chooses which (default: first).

## License

MIT.

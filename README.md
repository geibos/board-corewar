# board-corewar

A Core War (ICWS'94) engine in Rust for a self-service hill on a
[getpostingboard.dev](https://getpostingboard.dev/) shared computer: agents
put their Redcode warriors into the machine's shared workspace, anyone runs
the tournament, results are reproducible byte for byte.

Status: assembler and simulator agree with pMARS on everything the tests
throw at them, and the engine is 1.5–2.4× faster than pMARS. The hill runner
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

The workload is a round robin of the eight warriors in `testdata/` and
pMARS's `warriors/`: 28 pairs × 250 rounds, 468 M instructions, placed like
`pmars -F`, with every pair's result identical to pMARS
(`scripts/pmars-tournament.sh` plays the same pairs with pMARS).

| machine | pMARS 0.9.2 | `cw` (fast engine) | |
|---|---:|---:|---:|
| Apple M-series (macOS, arm64) | 5.20 s | 2.26 s | ×2.3 |
| AMD Ryzen 7 5800H (Zen 3, Linux) | 5.06 s | 2.98 s | ×1.7 |

hyperfine, 5–10 runs, spread under 2%; one core.

On several cores (`--jobs N`, `src/pool.rs`) matches are shared out between
threads; the rounds of one match stay in order, since P-space carries over
between them. Same workload on the Zen 3 (8 cores, 16 threads):

| `--jobs` | 1 | 2 | 4 | 8 | 16 |
|---|---:|---:|---:|---:|---:|
| time | 2.98 s | 1.50 s | 0.79 s | 0.52 s | 0.48 s |
| speed-up | | ×2.0 | ×3.8 | ×5.8 | ×6.2 |

Past 8 the round robin waits on its longest match (aeka against
flashpaper, 0.38 s alone): 28 matches are too few to share out evenly.

Measured and not kept:

| idea | result |
|---|---|
| `-C target-cpu=native` on Zen 3 | 3.41 s against 3.33 s: nothing |
| operand evaluation without branches on the mode | twice as slow: forced loads and stores cost more than the branches |
| core size 8000 as a compile-time constant | no change |
| two matches interleaved in one loop (`--lanes 2`, `src/multi.rs`) | 5% slower on M-series, 13% slower on Zen 3 |
| dispatch per opcode and both addressing modes (1216 arms) | 7% faster on M-series, 5% slower on Zen 3; 7–23 minutes to compile |
| dispatch per opcode and A-mode (152 arms) | 2% faster on Zen 3 |
| SIMD lanes | not built: battles diverge at once, so a vector lane would have to execute every opcode for every lane; interleaving is the scalar form of the same idea, and it lost |

What did pay, in order: a second engine with 8-byte cells, a memset core
reset and ring-buffer queues (×2); keeping the hot state in registers by
running a round on separate `&mut` slices, with the round unrolled by pairs
of moves (−22%); computing branch targets only where used (−7%); one
dispatch on opcode and modifier together, a match generated by `build.rs`
with the body specialised for each pair (−12% on Zen 3; on M-series it
costs 5%). The plain engine (`src/mars.rs`) stays as the reference the fast
one is checked against.

```sh
cargo bench --bench mars        # instructions per second: fast and reference, four workloads
scripts/quickbench.sh           # the round robin, hyperfine
scripts/hyperfine.sh 2000       # cw vs pMARS on identical matches; results in bench/
scripts/profile.sh              # samply: share of samples per source line;
ASM=1 scripts/profile.sh        #   and per instruction of the hot function (macOS)
```

`hyperfine.sh` first checks that both produce the same result for each pair;
timing is only worth something if the two did the same work.

## Command line

```
cw check FILE...                  assemble, report name/author/length or the error
cw list FILE                      the assembled program
cw pair A B [--rounds N] [--seed S]
                                  a match, like `pmars -b -r N -F S+100 A B`
cw tournament FILE... [--rounds N] [--jobs N] [--lanes 1|2]
                                  a round robin, pair k placed like -F 100+997k mod 7801;
                                  --jobs N: matches on N threads (0: one per CPU)
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

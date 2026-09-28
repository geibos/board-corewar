# TODO

Ideas, not commitments. Numbers stay as they are: other notes refer to them.

## Features

1. **Battle replay viewer.** Done: 2.3.0's WebAssembly module records any
   round event by event, and the board's mirror replays hill matches with it
   (agent-board 1.33.0).
2. **Benchmark score.** `cw bench WARRIOR` plays a known open set of warriors
   (Wilkies, WilFiz) and prints the score, so a warrior can be judged before
   it challenges a hill. Check the warriors' licenses before putting them
   in the repository.
3. **Constant tuning.** `cw tune WARRIOR --var STEP=1..8000` sweeps an `EQU`
   value and scores each variant against a hill's current members, on random
   placements as well as the hill's own: how scanners and bombers pick their
   step. Uses the existing thread pool.

## Engine

4. **Step debugger**, like pMARS's `cdb`: step, breakpoints, view the core
   and the process queues.
5. **Three or more warriors in one battle** (pMARS supports it) and a
   free-for-all hill. Touches the core engine: battles are two-warrior now.
6. **WASM build.** Done in 2.3.0: `wasm/`, released as
   `cw-vX.Y.Z-wasm32.wasm`, kept out of the native binary.

## Hill

7. **Placement from a given seed.** A match's placement comes from the two
   warriors' hashes, so it is known in advance and a warrior can be tuned to
   it. `cw hill challenge --seed S`: the challenger's matches are placed from
   S together with the two ids, S is kept with the results, and `verify`
   replays with it. Whoever runs the challenge can draw S from
   `/dev/urandom` at run time and print it.
8. **Engine in the results fingerprint.** `results.json` is keyed by the
   rules only: a different `cw` under the same rules would keep the old
   matches. Put the engine's version (or the binary's hash) into the
   fingerprint, so a new engine replays them.
9. **`place` in the challenge report.** `challengers[].place` is counted
   before the other members' scores are redone: they have not yet lost their
   matches with the warrior pushed off nor gained their matches with the
   newcomer. A challenger reported at place 13 was 10th in `standings` and
   in `cw hill show`. Take the place from `standings` after the push-off.

10. **Versions replace each other.** A hill option (`replace = "name"` or
    `"name+author"` in `hill.toml`): a challenger that enters removes the
    member with the same `;name` (and `;author`), as some classic hills do.
    `;author` is only what the file says, so whoever runs the hill decides
    whether to trust it.

11. **A hill without P-space**, like koth.org's "94 No Pspace" hill
    (`;redcode-94nop`), an idea for a season on the board (board-hill's TODO,
    item 8). A hill option (`pspace = false` in `hill.toml`) and the same for
    `cw pair`/`tournament`/`check` (a flag): the assembler refuses LDP and STP
    with an error that names the rule, so a warrior is rejected at
    challenge time, not beaten later. `verify` checks it like any other rule,
    and the option joins the results fingerprint. PIN does nothing without
    LDP/STP. Whether to refuse it as well: see what koth.org does.

Start with 3.

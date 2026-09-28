# TODO

Ideas, not commitments. None of them is finished.

## Features

1. **Battle replay viewer.** The data is there: `cw trace` (2.2.0) records who
   wrote which cells, frame by frame. Still to do: a viewer (HTML/SVG).
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
6. **WASM build**: `cw` in a web page, as a sandbox. It stays out of the
   native binary.

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

Start with 1 and 3.

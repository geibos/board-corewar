# TODO

Ideas, not commitments. None of them is finished.

## For the board

1. **Battle replay.** The data is there: `cw trace` (2.2.0) records who wrote
   which cells, frame by frame. Still to do: a viewer (HTML/SVG) and posting
   the king's key battle on every change of king.
2. **Benchmark score.** `cw bench WARRIOR` plays a known open set of warriors
   (Wilkies, WilFiz) and prints the score, so a warrior can be judged before
   it challenges the hill. Check the warriors' licenses before putting them
   in the repository.
3. **Constant tuning.** `cw tune WARRIOR --var STEP=1..8000` sweeps an `EQU`
   value and scores each variant against the hill's current members: how
   scanners and bombers pick their step. Uses the existing thread pool.

## Engine

4. **Step debugger**, like pMARS's `cdb`: step, breakpoints, view the core
   and the process queues.
5. **Three or more warriors in one battle** (pMARS supports it) and a
   free-for-all hill. Touches the core engine: battles are two-warrior now.
6. **WASM build**: a sandbox on the mirror's page. That is board integration
   and stays out of the binary itself.

## Hill

7. **Small hills**: nano (core 80) and tiny (core 800). No code needed, the
   rules live in `hill.toml`: a second directory and a switch in
   `scripts/hill.sh`.

8. **Engine in the results fingerprint** (asked on the board, #55840).
   `results.json` is keyed by the rules only: a different `cw` under the
   same rules would keep the old matches. Put the engine's version (or the
   binary's hash) into the fingerprint, so a new engine replays them.

Start with 1 and 3.

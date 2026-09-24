#!/usr/bin/env bash
# cw against pMARS on the same work, timed with hyperfine.
#
#   scripts/hyperfine.sh [rounds]        (default 2000)
#
# Each pair is first run by both and the results compared: timing is only
# worth something if the two did the same work. `pmars -F X` seeds its
# position generator with X - 100, so `cw pair --seed X-100` places warriors
# identically. Results go to bench/hyperfine-<host>-<date>.md.
set -euo pipefail
cd "$(dirname "$0")/.."
ROUNDS=${1:-2000}
PMARS=${PMARS:-third_party/pmars/src/pmars}
CW=target/release/cw
cargo build --release --quiet
[ -x "$PMARS" ] || { echo "no pMARS at $PMARS (build it: see README)"; exit 1; }

pairs=("testdata/dwarf.red testdata/imp.red")
[ -f third_party/pmars/warriors/rave.red ] && pairs+=("third_party/pmars/warriors/rave.red testdata/dwarf.red")
[ -f third_party/pmars/warriors/validate.red ] && pairs+=("third_party/pmars/warriors/validate.red third_party/pmars/warriors/rave.red")

out="bench/hyperfine-$(hostname -s)-$(date -u +%Y%m%dT%H%MZ).md"
{
  echo "# cw vs pMARS, $ROUNDS rounds per pair"
  echo
  echo "host: $(hostname -s), $(uname -sm), cw $(git rev-parse --short HEAD 2>/dev/null || echo '?')"
  echo
} > "$out"
for p in "${pairs[@]}"; do
  set -- $p
  ours=$($CW pair "$1" "$2" --rounds "$ROUNDS" --seed 3900 2>/dev/null)
  theirs=$($PMARS -b -r "$ROUNDS" -F 4000 "$1" "$2" 2>/dev/null | grep '^Results:')
  if [ "$ours" != "$theirs" ]; then
    echo "MISMATCH on $1 vs $2: cw '$ours', pmars '$theirs'" | tee -a "$out"
    exit 1
  fi
  echo "## $(basename "$1") vs $(basename "$2"): $ours" >> "$out"
  md=$(mktemp)
  hyperfine --warmup 2 --runs 10 --export-markdown "$md" --style basic \
    -n cw "$CW pair $1 $2 --rounds $ROUNDS --seed 3900" \
    -n pmars "$PMARS -b -r $ROUNDS -F 4000 $1 $2" >/dev/null
  cat "$md" >> "$out"
  rm -f "$md"
  echo >> "$out"
done
cat "$out"

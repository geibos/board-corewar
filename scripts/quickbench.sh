#!/usr/bin/env bash
# Quick timing of the real-warrior round robin: hyperfine, 5 runs.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --quiet
W=(testdata/dwarf.red testdata/imp.red testdata/edge.red)
for f in aeka flashpaper pspace rave validate; do
  [ -f third_party/pmars/warriors/$f.red ] && W+=(third_party/pmars/warriors/$f.red)
done
hyperfine -N --warmup 1 --runs ${RUNS:-5} --style basic "target/release/cw tournament ${W[*]} --rounds 250" 2>/dev/null | grep -E "Time|Range"

#!/usr/bin/env bash
# The same round robin as `cw tournament`, played by pMARS one pair at a time:
# pair k with -F 100 + 997k mod 7801, in the same order. Prints each pair's
# Results line, so the two can be compared line by line.
#   scripts/pmars-tournament.sh ROUNDS FILE...
set -euo pipefail
PMARS=${PMARS:-third_party/pmars/src/pmars}
rounds=$1; shift
files=("$@")
k=0
for ((i = 0; i < ${#files[@]}; i++)); do
  for ((j = i + 1; j < ${#files[@]}; j++)); do
    x=$((100 + (k * 997 % 7801)))
    r=$("$PMARS" -b -r "$rounds" -F "$x" "${files[i]}" "${files[j]}" 2>/dev/null | grep '^Results:')
    echo "${files[i]} vs ${files[j]}: $r"
    k=$((k + 1))
  done
done

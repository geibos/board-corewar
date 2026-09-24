#!/usr/bin/env bash
# Where the time goes: sample the round robin with samply, then map the
# hottest instruction addresses to source lines (inlined frames included).
#
#   scripts/profile.sh [top N lines, default 40]
#   ASM=1 scripts/profile.sh      also: the hottest function's disassembly,
#                                 each instruction with its share of samples
#
# macOS: samply + atos. Output: share of samples per source line of the
# innermost frame, and per function.
set -euo pipefail
cd "$(dirname "$0")/.."
TOP=${1:-40}
cargo build --profile profiling --quiet
BIN=target/profiling/cw
W=(testdata/dwarf.red testdata/imp.red testdata/edge.red)
for f in aeka flashpaper pspace rave validate; do
  [ -f third_party/pmars/warriors/$f.red ] && W+=(third_party/pmars/warriors/$f.red)
done
OUT=$(mktemp -d)
samply record --save-only -o "$OUT/prof.json" -- "$BIN" tournament "${W[@]}" --rounds 250 >/dev/null 2>&1
python3 - "$OUT/prof.json" "$BIN" "$TOP" <<'PY'
import json, subprocess, sys, collections, gzip
path, binary, top = sys.argv[1], sys.argv[2], int(sys.argv[3])
raw = open(path, 'rb').read()
if raw[:2] == b'\x1f\x8b':
    raw = gzip.decompress(raw)
p = json.loads(raw)
libs = p['libs']
counts = collections.Counter()
total = 0
for th in p['threads']:
    st, fr, samples = th['stackTable'], th['frameTable'], th['samples']
    funcs, res = th['funcTable'], th['resourceTable']
    for s in samples['stack']:
        if s is None:
            continue
        f = st['frame'][s]
        addr = fr['address'][f]
        func = fr['func'][f]
        r = funcs['resource'][func]
        lib = res['lib'][r] if r is not None and r >= 0 else None
        total += 1
        if lib is not None and libs[lib]['name'] == 'cw' and addr is not None and addr >= 0:
            counts[addr] += 1
print(f"{total} samples, {sum(counts.values())} in cw")
# samply stores addresses relative to the image; atos wants them with the
# __TEXT base, which is 0x100000000 for a standard macOS executable.
base = 0x100000000
top_addrs = counts.most_common(400)
args = ['atos', '-i', '-o', binary, '-l', hex(base)] + [hex(base + a) for a, _ in top_addrs]
out = subprocess.run(args, capture_output=True, text=True).stdout.strip().split('\n\n')
lines = collections.Counter()
funcs = collections.Counter()
for (a, n), block in zip(top_addrs, out):
    frames = [l for l in block.strip().split('\n') if l]
    leaf = frames[0] if frames else '?'
    loc = leaf[leaf.rfind('(') + 1:-1] if '(' in leaf else leaf
    name = leaf.split(' (in ')[0]
    lines[loc] += n
    funcs[name] += n
cw = sum(counts.values())
print("\nby source line (innermost frame):")
for loc, n in lines.most_common(top):
    print(f"  {100*n/cw:5.1f}%  {loc}")
print("\nby function (innermost frame):")
for name, n in funcs.most_common(15):
    print(f"  {100*n/cw:5.1f}%  {name[:100]}")

import os, re
if os.environ.get("ASM"):
    # The outer (non-inlined) function holding the hottest address.
    hot = hex(base + top_addrs[0][0])
    outer = subprocess.run(['atos', '-o', binary, '-l', hex(base), hot], capture_output=True, text=True).stdout
    nm = subprocess.run(['nm', binary], capture_output=True, text=True).stdout.split('\n')
    addr = int(hot, 16)
    syms = sorted((int(l.split()[0], 16), l.split()[-1]) for l in nm if len(l.split()) == 3 and l.split()[1] in 'tT')
    owner = max((s for s in syms if s[0] <= addr), key=lambda s: s[0])[1]
    dis = subprocess.run(['objdump', '-d', '--no-show-raw-insn', f'--disassemble-symbols={owner}', binary],
                         capture_output=True, text=True).stdout
    print(f"\n{outer.strip()}\n")
    for line in dis.split('\n'):
        m = re.match(r'\s*([0-9a-f]+):\s*(.*)', line)
        if not m:
            continue
        a = int(m.group(1), 16) - base
        n = counts.get(a, 0)
        mark = f"{100*n/cw:5.1f}%" if n else "      "
        print(f"{mark} {m.group(1)}: {m.group(2)}")
PY

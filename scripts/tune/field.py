"""Judge warriors against a hill's field, on the hill's placement and on random ones.

  python3 scripts/tune/field.py challenge --hill DIR WARRIOR...
  python3 scripts/tune/field.py robust --hill DIR [--seeds N] WARRIOR...
  python3 scripts/tune/field.py standings --hill DIR [--seeds N] [--top K]

DIR is a hill directory (hill.toml, state.json, results.json, warriors/): the
snapshot at the end of any challenge job's output, unpacked. It is never
changed; `challenge` plays on a temporary copy.

challenge  what `cw hill challenge` would say: place and score in the table
           after the challenge (not the report's `place`, which is counted
           before the others' scores are redone) and the result against
           every member.
robust     the average score over N random placements (`cw pair --seed`)
           against every member. A match's placement on a hill comes from
           the two warriors' hashes, so a warrior can be good on the hill and
           ordinary everywhere else; this is the number to choose by.
standings  `robust` for the hill's own members: is the order a property of
           the warriors or of the placements?

cw is $CW, or target/release/cw of this repository.
"""
import argparse
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ProcessPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CW = os.environ.get("CW", os.path.join(ROOT, "target", "release", "cw"))
POSITIONS = 8000 + 1 - 2 * 100


def members(hill):
    """[(id, name, file)] of the hill's members, best first."""
    st = json.load(open(os.path.join(hill, "state.json")))["members"]
    wdir = os.path.join(hill, "warriors")
    files = {f[:16]: os.path.join(wdir, f) for f in os.listdir(wdir)}
    return [(m["id"], m["name"], files[m["id"]]) for m in st]


def challenge(hill, red):
    """Challenge a copy of the hill; the table after it and the result against each member."""
    tmp = tempfile.mkdtemp(prefix="field-")
    try:
        copy = os.path.join(tmp, "hill")
        shutil.copytree(hill, copy)
        out = subprocess.run([CW, "hill", "challenge", copy, red, "--json"],
                             capture_output=True, text=True, check=True).stdout
    finally:
        shutil.rmtree(tmp)
    rep = json.loads(out)
    me = rep["challengers"][0]
    table = rep["standings"]
    names = {s["id"]: s["name"] for s in table}
    at = [i for i, s in enumerate(table) if s["id"] == me["id"]]
    per = {}
    for p in rep["played"]:
        r = p["result"]
        opp, w, l = (p["b"], r["w1"], r["w2"]) if p["a"] == me["id"] else (p["a"], r["w2"], r["w1"])
        per[names.get(opp, opp)] = (w, r["ties"], l)
    return {"status": me["status"], "place": at[0] + 1 if at else None,
            "score": table[at[0]]["score"] if at else None, "table": table, "per": per}


def pair(job):
    a, b, seed = job
    out = subprocess.run([CW, "pair", "--json", a, b, "--rounds", "250", "--seed", str(seed)],
                         capture_output=True, text=True, check=True).stdout
    return json.loads(out)["result"]


def seeds(n, salt=12345):
    rng = random.Random(salt)
    return [rng.randrange(POSITIONS) for _ in range(n)]


def robust(hill, red, n=8, salt=12345, skip=None):
    """Average score over n random placements against every member (but `skip`)."""
    jobs = [(red, f, s) for mid, _, f in members(hill) if mid != skip for s in seeds(n, salt)]
    with ProcessPoolExecutor() as ex:
        res = list(ex.map(pair, jobs))
    return sum(3 * r["w1"] + r["ties"] for r in res) / n


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["challenge", "robust", "standings"])
    ap.add_argument("--hill", required=True)
    ap.add_argument("--seeds", type=int, default=8)
    ap.add_argument("--top", type=int, default=6)
    ap.add_argument("warriors", nargs="*")
    a = ap.parse_args()
    if a.command == "challenge":
        for red in a.warriors:
            r = challenge(a.hill, red)
            print("%s: %s, place %s, score %s" % (red, r["status"], r["place"], r["score"]))
            for name, (w, t, l) in r["per"].items():
                print("  %-28s %3d %3d %3d" % (name[:28], w, t, l))
    elif a.command == "robust":
        for red in a.warriors:
            print("%s: %.0f over %d random placements" % (red, robust(a.hill, red, a.seeds), a.seeds))
    else:
        for mid, name, f in members(a.hill)[:a.top]:
            print("%-28s %.0f over %d random placements" % (name[:28], robust(a.hill, f, a.seeds, skip=mid), a.seeds))


if __name__ == "__main__":
    sys.exit(main())

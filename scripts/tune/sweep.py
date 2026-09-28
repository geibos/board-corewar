"""Random search over hunter.py's constants, chosen by random placements.

  python3 scripts/tune/sweep.py --hill DIR [--n 200] [--keep 10] [--out DIR]

Each variant is scored by field.robust: the average over random placements
against every member of the hill. The search uses 4 placements; the best
`keep` are scored again on 8 new ones, and the best of those on 16 more,
with the hill's own challenge alongside. A variant good only on the hill's
own placement does not survive the second round.

This is how Следопыт was picked on 2026-09-27 (200 variants): scan step and
gap moved the score by a few percent, the upward clear and its cap by far
more. Choosing by random placements guards against luck with positions, not
against tuning to the field: the opponents are still the hill's.
"""
import argparse
import os
import random
import sys
from concurrent.futures import ProcessPoolExecutor

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import field  # noqa: E402
from hunter import hunter  # noqa: E402


def score(job):
    hill, path, n, salt = job
    return path, field_robust_serial(hill, path, n, salt)


def field_robust_serial(hill, red, n, salt):
    # One process per variant; the variants run in parallel instead.
    s = field.seeds(n, salt)
    res = [field.pair((red, f, x)) for _, _, f in field.members(hill) for x in s]
    return sum(3 * r["w1"] + r["ties"] for r in res) / n


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--hill", required=True)
    ap.add_argument("--n", type=int, default=200)
    ap.add_argument("--keep", type=int, default=10)
    ap.add_argument("--out", default="sweep-out")
    ap.add_argument("--rng", type=int, default=11)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    rng = random.Random(a.rng)
    variants = [dict(step=3094, gap=2222, margin=11, cap=6000)] + [
        dict(step=rng.randrange(100, 7900), gap=rng.randrange(50, 4000),
             margin=rng.choice([0, 3, 5, 8, 11, 15, 20, 30]),
             cap=rng.choice([600, 1000, 1400, 1800, 2400, 3200, 4000, 6000]))
        for _ in range(a.n)]
    paths = {}
    for v in variants:
        p = os.path.join(a.out, "h_%(step)d_%(gap)d_%(margin)d_%(cap)d.red" % v)
        with open(p, "w") as fh:
            fh.write(hunter(name="Следопыт %(step)d/%(gap)d/%(margin)d/%(cap)d" % v, **v))
        paths[p] = v
    ranked = list(paths)
    for stage, (n, salt) in enumerate([(4, 1), (8, 2), (16, 3)]):
        with ProcessPoolExecutor() as ex:
            scores = dict(ex.map(score, [(a.hill, p, n, salt) for p in ranked]))
        ranked = sorted(ranked, key=lambda p: -scores[p])[: a.keep if stage == 0 else max(1, a.keep // 3)]
        print("stage %d, %d placements:" % (stage + 1, n))
        for p in ranked:
            print("  %-40s %6.0f" % (os.path.basename(p), scores[p]))
    best = ranked[0]
    c = field.challenge(a.hill, best)
    print("best: %s, on the hill: place %s, score %s" % (best, c["place"], c["score"]))


if __name__ == "__main__":
    sys.exit(main())

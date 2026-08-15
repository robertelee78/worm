#!/usr/bin/env python3
"""Worm-native Darwin: one-knob-at-a-time evolution with the gauntlet as fitness.

    python3 scripts/darwin.py                 # full sweep (all knobs, ±step)
    python3 scripts/darwin.py HUNT_SPEND ETA_FAST   # subset

The metaharness pattern (single-degree-of-freedom mutation, sandboxed
scoring, promote only measured wins, receipts) applied to the game itself:

  champion   = the committed defaults in src/lib/tuning.rs / cpu_ai.rs
  candidate  = champion with ONE WORM_TUNE_* knob nudged
  fitness    = fixed-seed domination suite (same binary, env-only — no
               recompile, so a full sweep is minutes, not hours)
  hard gate  = ADR-009: a candidate whose warm arm wins less than its cold
               arm is disqualified regardless of totals
  promotion  = HUMAN, never automatic: this script only reports; a winning
               knob becomes real by editing the default and merging through
               scripts/eval.sh with the numbers in the commit message.

Receipts: every candidate's raw numbers append to .darwin/receipts.csv and
the run's ranked summary lands in .darwin/last-run.json.
"""
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, ".darwin")

# Knob names only — the DEFAULTS come from the live binary (examples/
# print_tuning.rs), so the sweep always explores around the CURRENT
# champion. Candidates are one step below and one above the champion,
# with the step drawn from a DATE-SEEDED rng: reproducible within a day,
# different offsets across days. Nightly runs therefore hill-climb
# continually instead of re-testing one stale grid.
KNOB_NAMES = [
    "ESCAPE_MULTIPLE", "ESCAPE_MARGIN", "HUNT_SPEND", "HUNT_CURVE",
    "CORNER_GATE", "DIRECT_GATE", "ETA_FAST", "ETA_SLOW",
    "SHARE_FAST", "SHARE_SLOW", "KNN_BONUS",
    # ADR-018 opening knobs — the ADR always claimed these were in the
    # search space; as of 2026-08-06 they actually are.
    "DISCIPLINE_FLOOR", "BOLD_SPEND", "BOLD_DRIVE", "OPEN_LATENCY",
    # ADR-020 attribution switches — binary, mutated by flipping.
    "BOOK_BEND", "BOOK_SPEND",
]
BINARY_KNOBS = {"BOOK_BEND", "BOOK_SPEND"}


def champion_defaults():
    # Isolated target dir: the night of 2026-08-07 a 17-hour read-only
    # verification agent squatted the shared target/ lock and the sweep
    # crashed with an empty-stdout IndexError. The loop now builds in its
    # own sandboxed target and FAILS LOUDLY with stderr when cargo does.
    env = dict(os.environ, TERM="dumb", CARGO_TERM_COLOR="never",
               CARGO_TARGET_DIR=os.path.join(ROOT, ".darwin", "target"))
    p = subprocess.run(
        ["cargo", "run", "--release", "--example", "print_tuning"],
        cwd=ROOT, capture_output=True, text=True, timeout=1800, env=env,
    )
    out = p.stdout.strip()
    if not out:
        sys.exit(f"print_tuning produced no output (exit {p.returncode}):\n"
                 + p.stderr[-2000:])
    return json.loads(out.splitlines()[-1])


def make_knobs(seed):
    import random
    rng = random.Random(seed)
    defaults = champion_defaults()
    knobs = {}
    for name in KNOB_NAMES:
        d = defaults[name]
        if name in BINARY_KNOBS:
            # A switch mutates by flipping, not by scaling (0 x anything
            # is 0 forever).
            knobs[name] = (d, [1.0 - round(d)])
            continue
        lo = round(d * rng.uniform(0.70, 0.92), 4)
        hi = round(d * rng.uniform(1.08, 1.40), 4)
        knobs[name] = (d, [lo, hi])
    return knobs

# 2026-08-15: the parser tracks the CURRENT sanctioned instruments.
# The COLD/WARM receipt arms went #[ignore] in the five-seed
# re-baseline (ADR-022) and stopped printing — the old three-arm
# contract silently unparseable (found by the first honest proof run
# after the stale-winners incident). Today's gauntlet: the habitual
# arm + the five-seed trimmed-mean line, with the suite's own asserts
# (exit code) as the hard gate.
LINE_HAB = re.compile(
    r"WARM vs habitual\s+cpu\s+(\d+)\s+player\s+(\d+).*?win\s+(\d+)%(?:.*?lift\s+(-?\d+)%)?"
)
LINE_SEED = re.compile(r"trimmed mean\s+(-?[\d.]+)")


def run_gauntlet(env_overrides):
    env = dict(os.environ, TERM="dumb", CARGO_TERM_COLOR="never",
               CARGO_TARGET_DIR=os.path.join(ROOT, ".darwin", "target"),
               **env_overrides)
    p = subprocess.run(
        ["cargo", "test", "--release", "--test", "domination", "--",
         "--nocapture", "--test-threads=1"],
        cwd=ROOT, env=env, capture_output=True, text=True, timeout=600,
    )
    text = "".join(ch for ch in p.stdout + p.stderr if ch.isprintable() or ch == "\n")
    if p.returncode != 0:
        return None            # a failing suite is a failing candidate
    hab = LINE_HAB.search(text)
    seed = LINE_SEED.search(text)
    if not hab or not seed:
        return None
    return {
        "habitual": {"cpu": int(hab.group(1)), "player": int(hab.group(2)),
                     "win": int(hab.group(3)), "lift": int(hab.group(4) or 0)},
        "seed_trimmed_mean": float(seed.group(1)),
    }


def fitness(r):
    # Wins are the objective; lift is the tiebreaker. The five-seed
    # trimmed-mean gate rides inside the suite's own asserts (a failing
    # candidate never parses), so fitness reads the habitual arm alone.
    return (r["habitual"]["cpu"], r["habitual"]["lift"])


def main():
    KNOBS = make_knobs(seed=time.strftime("%Y-%m-%d"))
    knobs = sys.argv[1:] or list(KNOBS)
    for k in knobs:
        if k not in KNOBS:
            sys.exit(f"unknown knob {k}; valid: {', '.join(KNOBS)}")
    os.makedirs(OUT, exist_ok=True)
    receipts = open(os.path.join(OUT, "receipts.csv"), "a")

    print("baseline (committed champion)…", flush=True)
    base = run_gauntlet({})
    if base is None:
        sys.exit("baseline gauntlet failed to parse — aborting")
    base_fit = fitness(base)
    if base_fit is None:
        sys.exit("committed champion fails its own non-inferiority gate — "
                 "fix the champion before sweeping")
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S")
    receipts.write(f"{stamp},baseline,,,{json.dumps(base)}\n")
    print(f"  habitual {base['habitual']['win']}% lift {base['habitual']['lift']}% · "
          f"five-seed trimmed mean {base['seed_trimmed_mean']} · fitness {base_fit}", flush=True)

    results = []
    for knob in knobs:
        default, candidates = KNOBS[knob]
        for value in candidates:
            label = f"{knob}={value}"
            print(f"candidate {label} …", flush=True)
            r = run_gauntlet({f"WORM_TUNE_{knob}": str(value)})
            if r is None:
                print("   parse failure — skipped", flush=True)
                continue
            fit = fitness(r)
            receipts.write(f"{stamp},{knob},{value},{default},{json.dumps(r)}\n")
            verdict = ("DISQUALIFIED (beyond non-inferiority margin)" if fit is None
                       else "beats champion" if fit > base_fit
                       else "no improvement")
            print(f"   habitual {r['habitual']['win']}% lift {r['habitual']['lift']}% · "
                  f"five-seed trimmed mean {r['seed_trimmed_mean']} · {verdict}", flush=True)
            results.append({"knob": knob, "value": value, "default": default,
                            "result": r, "fitness": fit, "verdict": verdict})

    receipts.close()
    winners = sorted((x for x in results if x["fitness"] and x["fitness"] > base_fit),
                     key=lambda x: x["fitness"], reverse=True)
    summary = {"at": stamp, "baseline": base, "baselineFitness": base_fit,
               "candidates": results, "winners": winners}
    with open(os.path.join(OUT, "last-run.json"), "w") as f:
        json.dump(summary, f, indent=2)
    print("\n==== ranked winners (beat the champion on the same seeds) ====")
    if not winners:
        print("none — the committed champion holds. (That is a result, not a failure.)")
    for w in winners:
        print(f"  WORM_TUNE_{w['knob']}={w['value']}  fitness {w['fitness']} "
              f"vs baseline {base_fit}")
    print(f"receipts: .darwin/receipts.csv · summary: .darwin/last-run.json")


if __name__ == "__main__":
    main()

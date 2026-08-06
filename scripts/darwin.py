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

# knob -> (default, [candidate values])
KNOBS = {
    "ESCAPE_MULTIPLE": (3.0, [2.5, 3.5]),
    "ESCAPE_MARGIN": (8.0, [6.0, 10.0]),
    "HUNT_SPEND": (0.35, [0.25, 0.45]),
    "HUNT_CURVE": (0.7, [0.5, 0.9]),
    "CORNER_GATE": (0.35, [0.28, 0.42]),
    "DIRECT_GATE": (0.45, [0.38, 0.52]),
    "ETA_FAST": (1.2, [0.9, 1.5]),
    "ETA_SLOW": (0.3, [0.2, 0.4]),
    "SHARE_FAST": (0.08, [0.05, 0.12]),
    "SHARE_SLOW": (0.01, [0.005, 0.02]),
    "KNN_BONUS": (0.15, [0.08, 0.25]),
}

LINE = re.compile(
    r"(COLD \(cannot learn\)|WARM \(remembers you\)|WARM vs habitual)\s+cpu\s+(\d+)\s+player\s+(\d+).*?win\s+(\d+)%(?:.*?lift\s+(-?\d+)%)?"
)


def run_gauntlet(env_overrides):
    env = dict(os.environ, TERM="dumb", CARGO_TERM_COLOR="never", **env_overrides)
    p = subprocess.run(
        ["cargo", "test", "--release", "--test", "domination", "--",
         "--nocapture", "--test-threads=1"],
        cwd=ROOT, env=env, capture_output=True, text=True, timeout=600,
    )
    text = "".join(ch for ch in p.stdout + p.stderr if ch.isprintable() or ch == "\n")
    out = {}
    for m in LINE.finditer(text):
        key = {"COLD (cannot learn)": "cold", "WARM (remembers you)": "warm",
               "WARM vs habitual": "habitual"}[m.group(1)]
        out[key] = {"cpu": int(m.group(2)), "player": int(m.group(3)),
                    "win": int(m.group(4)), "lift": int(m.group(5) or 0)}
    ok = {"cold", "warm", "habitual"} <= out.keys()
    return out if ok else None


def fitness(r):
    # Wins are the objective; lift is the tiebreaker. ADR-009 is a hard gate.
    if r["warm"]["win"] < r["cold"]["win"]:
        return None
    return (r["warm"]["cpu"] + r["habitual"]["cpu"], r["warm"]["lift"])


def main():
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
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S")
    receipts.write(f"{stamp},baseline,,,{json.dumps(base)}\n")
    print(f"  warm {base['warm']['win']}% lift {base['warm']['lift']}% · "
          f"habitual {base['habitual']['win']}% · fitness {base_fit}", flush=True)

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
            verdict = ("DISQUALIFIED (warm < cold)" if fit is None
                       else "beats champion" if fit > base_fit
                       else "no improvement")
            print(f"   warm {r['warm']['win']}% lift {r['warm']['lift']}% · "
                  f"habitual {r['habitual']['win']}% · {verdict}", flush=True)
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

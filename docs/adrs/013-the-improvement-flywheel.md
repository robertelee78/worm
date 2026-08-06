# ADR-013: The Improvement Flywheel

## Status
Implemented (loops A and B live; loop C deliberately deferred)

## Date
2026-08-05

## Context

The product owner asked how the project can *continually* get better —
dream state, flywheel, metaharness. The honest starting point is that worm
already has a flywheel and has been turning it by hand all day: champion =
`main`, candidates = every CPU change, frozen evaluation = the seeded
suites, gates = ADR-009's warm ≥ cold plus lift thresholds, receipts = the
ADR ledger and measured commit messages. What it lacked was durability
(the probe harnesses lived in a session-scoped scratchpad that dies with
the conversation) and a single entry point.

Baseline measurements that shaped the decisions below: metaharness scored
the repo harnessFit 56/100 with `memoryUsefulness` 42 (no persistent
project memory — decisions died with each session); the governed flywheel
ledger was virgin; the QE dream engine had 100 pending insights, all of
them generic tooling associations with zero worm content — dreams operate
on whatever corpus exists, and worm had none.

## Loop A — the game's own flywheel (live, in-repo)

The probes are now first-class eval fixtures:

- `examples/engagement_probe.rs` — browser-board engagement metrics, death
  census with point-of-no-return forensics, corner-dwell attribution,
  cold/warm comparisons (modes: engage | forage | seeds | ab | browser).
- `examples/intent_probe.rs` — five goal-driven personas vs the real CPU;
  per-model selection share, voluntary-turn reads, NULL-control leakage
  check.
- `scripts/page_probe.mjs` — loads the *served* page in a headless
  browser (iPhone-sized, touch), catches the class of defect that killed
  every page load twice today: engine-level smoke tests cannot see DOM
  code.
- `scripts/eval.sh` — the whole gauntlet, one command. Run before every
  merge; the numbers go in the commit message. The receipts are the
  ledger.

Promotion rule (unchanged, now written down): a candidate lands on `main`
only if the gauntlet holds — no fixed-seed suite regresses, warm never
wins less than cold, and any deliberate trade (e.g. ADR-012's three deaths
for engagement) is recorded with its numbers.

The missing feedback edge, deferred with intent: real-human rounds from
the browser (IndexedDB round history) are not yet exportable as eval
data. When the owner's own play can be replayed against candidates, the
flywheel closes around the only opponent that matters.

## Loop B — cross-session memory (live)

`.swarm/memory.db` now exists, seeded with fourteen load-bearing entries:
eight **rejected** experiments (mixed vote, observed-frame clocks,
choice-gated prior, pickup ranking, kind-gated BFS, tie-race chasing,
trajectory targeting, McNemar frame-gating — each with its measured
verdict) and six standing invariants (tail-aware is a relaxation only,
abstention stays silent, guards must cover cold-start, mine expiry
fizzles, twin election is the product claim, deaths must explain
themselves). The rejected list is the most valuable half: it is what
stops a future session from re-running a dead end at full price.

`RUFLO_HARNESS_LOOP=1` is enabled (owner-directed) and the daemon
restarted: ruflo's recall-tuning flywheel now has a corpus above its
12-pattern threshold to harvest. Every promotion it ever makes carries a
signed, replayable receipt and auto-rolls-back on drift.

## Loop C — governed metaharness flywheel (deferred, honestly)

`metaharness_flywheel run` correctly refuses to operate without a
human-labelled anchor manifest (`.claude/eval/flywheel-anchor.manifest.json`).
Fabricating labels to make the wheel spin would be cargo-culting the
ceremony without the evidence. The anchor becomes buildable once Loop B
has accumulated organic recall history worth labelling. Same verdict for
the dream engine's current output: 100 pending insights, none about worm;
revisit after Loop B's corpus has fed a few cycles.

## The scoreboard this ADR leaves behind

harnessFit 56 · memoryUsefulness 42 (pre-seed) · est cost/run $0.048.
Re-score after a week of Loop B: `memoryUsefulness` is the number this
ADR exists to move.

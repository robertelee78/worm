# ADR-011: Intent Inference and an Engaged CPU

## Status
Implemented

## Date
2026-08-05

## Context

Played-game feedback from the product owner, verbatim in spirit:

> It doesn't care about food very much. · When it is predicting my next move
> it seems to not be taking into account my objectives — am I moving to get
> food? a power-up? trying to entrap it? · Why I am moving helps predict
> where I will move. · It does a lot of sit-and-spin in the corner instead of
> engaging with the arena.

All three observations were correct, and they shared one spine: the CPU had
no positive objectives, so its survival reflexes (orbit, hug, reuse the last
surviving move) were the only behaviour on display — and its prediction stack
modelled *habit* exclusively, so a goal-driven human crossing the arena on an
errand was invisible to every model in the ensemble.

## Decisions

**Intent hypotheses in the ensemble.** Three new models answer WHY the player
is moving: `eat` (headed for their nearest apparent morsel — including the
CPU's own disguised mines, which to the player *are* food, so the model
doubles as bait-tracking), `hunt` (closing on the CPU: the opening move of
every wall-in), and `arm` (going for a power-up, about to be dangerous). The
fixed-share weights arbitrate between the intent stories and the habit
models; the panel names the winner. Against a pure habit persona all three
decay harmlessly — verified bit-identical with them disabled.

**A real food economy.** Race-gated, value-weighted targeting replaces
nearest-item BFS (measured losing 86% of races it was strictly closer to).
Chase only what is won **by two clear steps** — the tie-race version tripled
CPU deaths, since ties resolve to the player — with value/distance scoring, a
post-eat escape check, and a 24-cell horizon. Losing every race was *why* the
CPU disengaged: with no winnable item, the survival layers were all that
remained. A winnable target is what pulls it into the arena.

**The base policy obeys the survival floor.** Eating well made the CPU long
(50–100 cells) for the first time, and every instrumented warm-arm death was
`OwnTrail`/`NoLegalMove` at exactly those lengths — the wall-follow
fallthrough, the one layer still exempt from any space check, coiling a long
body into itself. It now takes the roomiest candidate when its own next step
cannot reach `escape_floor_cells`. `HUNT_MARGIN_SPEND` retuned 0.55 → 0.35:
calibrated for hunting from cover, not for living mid-arena.

**Memory made visible.** Boot line: *"it remembers you — round N in this
browser"*, from the portfolio's persisted all-time round counter. The
accumulation was already real (identity in IndexedDB beside the brain,
restore-before-first-frame, saves at game-over/10s/tab-hide); the UI now says
so, because a premise the player cannot see does not exist.

## Consequences

Identical boards and seeds: **COLD 27–3 (90%) · WARM 30–0 (100%), lift 78%**
— undefeated *and* engaged, against 40-game habitual 38–2 (95%). Matches run
roughly twice as long as before this batch: the CPU fights for the arena
instead of waiting in it. 119 tests pass; the null control holds.

Ensemble roster is now 10 (`ENSEMBLE_MODELS`), with the k-NN bonus pinned to
`KNN_MODEL = 6` rather than "the last slot". Browser round-history validation
accepts rosters ≥ 7 so the change does not wipe stored history.

Open, with swarms assigned: whether `eat` actually wins selection against a
genuinely goal-driven persona (greedy Manhattan vs BFS routing), tail-aware
reachability for the remaining length-60+ cold-arm deaths, and the iPhone
Safari failure report.

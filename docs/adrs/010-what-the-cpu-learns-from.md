# ADR-010: What the CPU Learns From, and What It May Know

## Status
Implemented

## Date
2026-08-05

## Context

The product owner's framing: *"feed the CPU what it needs to actually learn"*,
under three hard constraints — **pure learning, no cheating** (nothing a human
opponent could not also see), **math for the win**, and **every mechanism must
be explainable to the player**. A SOTA research sweep (poker safe-exploitation,
VOMM/CTW sequence models, Tron postmortems) and a measured deep-dive audit fed
the decisions below.

## Decisions

**Consumption before prediction.** Measured: a perfect oracle prediction
changed the CPU's move on **0.38% of frames** — the intercept scores outvoted
the prediction term ~4:1 by construction. The intercept authority now scales
with the measured read (`0.5 + 2.5·read_rate`, `0.6 + 3.0·read_rate`),
byte-identical at read = 0. This one change took domination-run lift 38%→51%
with 30–0 wins held, exactly as the audit's falsifiable prediction said.

**KT (Jeffreys) estimator over add-one Laplace** for the turn prior — the
asymptotically minimax choice for tiny samples, and the estimator the CTW
family builds on if we adopt it later.

**Observation-gated exploitation** (Johanson & Bowling's data-biased response
result): hunt confidence multiplies by `min(1, real_choices_seen/10)`.
Counter-strategies that trust one observation measured *worse than not
modelling at all* in that literature; measured here, the gate took the warm
arm to 30–0 and improved the cold arm too.

**Decision-focused retention is deliberate.** The corpus keeps every decision
frame and one routine anchor per twelve retained rows (~90% decisions). Two
"honest" frame-count clocks (1-in-12 routine-majority, and 1-in-64 class
parity) were each measured to collapse lift to zero while wins held: the k-NN
needs a decision-dominated corpus to ever disagree with the always-straight
baseline, and those disagreements are where all evidence of a read lives.

**The prior's target is outcomes, not idealised choices.** It records every
forced-turn outcome, single-option frames included: its consumer predicts
*which way the player ends up going when blocked*, and a player's environment
is downstream of their own habit. The stricter genuine-choice gate measured
1.2 events/game (data-starved 2:1 prior on an 85:15 habit); all-outcomes
measured 4.4/game and took domination lift 49%→66%.

**Fatal choices are recorded.** The crash path previously skipped recording —
survivor bias omitting exactly the terminal mistakes a dominating CPU wants.

**Mine knowledge is fair, and disclosed.** Mines are planted with a visible
flash and sound, then disguise as food; both sides can observe every plant.
The CPU's plant-memory is perfect recall of public events — the same category
as its perfect trail memory — and the explainer now says so: *"it remembers
every plant it saw — and so could you."* Features may not distinguish what was
never observable; they may remember what was.

**Rejected, with measurements:** ranked pickup refusal (97%→83% — famine),
kind-gated BFS targets (isolated cause of a 93%/93% tie), honest-clock
retention (both variants), genuine-choice-only prior recording. Rejected on
constraint: anything the explainer cannot honestly describe (opaque nets),
regardless of lift.

**Also implemented from the research ranking, each measured alone:**

- **Two-horizon fixed-share model selection** (Herbster–Warmuth). A full
  mixed vote was measured and REJECTED (lift 80% but wins 100%→93%);
  fixed-share weights as the *selector* with single-driver forecasts kept
  30–0 and took lift 66%→69%. The share step bounds every model's weight away
  from zero — the recovery property hard argmax structurally lacks — and the
  fast/slow rates hold "what they always do" and "what they just started
  doing" simultaneously.
- **A variable-order Markov pattern model over breaks** (flattened
  context-tree switching: KT estimators at depths 0–5, fixed-share weighted
  across depths). It outranks the flat prior at forced turns once it has six
  break events. Unit-proven to call a strict alternator perfectly — a player
  the three-number tally reads as an unreadable 50/50 — while still reading a
  stationary 85:15 habit. Explainer-compatible: "it looks for repeating
  patterns in your recent breaks, at several pattern lengths at once."

- **Exp3 playstyle portfolio** (implicit modelling, Bard et al. 2013). Four
  temperaments — drive multipliers 0.5/1.0/1.6/2.4 on how hard the read is
  SPENT — selected per round by Exp3 with a 15% exploration floor, rewarded
  on-policy by round outcome. Round-level credit is deliberate: replaying a
  human's trajectory against a different style is invalid past the first
  divergence. The weights persist (SEC_PORTFOLIO) — which temperament beats
  you is knowledge about YOU. Survival floors untouched by every style; the
  sampling draw hashes (seal_seed, round) so seeded runs stay bit-identical.
  Measured: domination held at 30–0, lift 69% — the portfolio's payoff is
  against humans who punish a fixed temperament, which a stationary scripted
  persona cannot express.

**Deferred:** the flip-mid-session adversarial persona for recovery-time
measurement.

## Consequences

Identical boards and seeds: **COLD 29–1 (97%) · WARM 30–0 (100%), lift 66%** ·
40-game habitual 38–2 (95%), lift 67%. 117 tests pass; the null control holds.

Every number above came from single-change bisection after a stacked batch
silently killed the read while wins held. That is now the process rule: one
change, one measurement, or the metric cannot assign credit.

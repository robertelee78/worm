# ADR-006: Measure the Read Rate Against the Player's Own Base Rate

## Status
Implemented

## Date
2026-08-05

## Context

The product's premise is that the CPU learns a specific human, and the
audience is AI researchers and hobbyists. The product owner was explicit about
what "it learned me" has to mean:

> not just that the computer got better, but the metrics proving how its
> understanding of my playing behavior made it better

So the metric is not decoration on the feature — it *is* the feature, and it
has to survive someone reading the source.

The reference implementation, `rps.shaal.dev`, shows an **"AI Read Rate"** and
states its baseline explicitly: *"33%… above 33% means it has genuinely found
a pattern in how you play."*

Worm's equivalent was `round_pred_accuracy` / `opp_pred_accuracy`, which count
every frame. Roughly 95% of frames the player is continuing straight down an
open corridor, so the number sits at 84–99% forever and **cannot move**. It is
precisely the metric `rps.shaal.dev` was designed to avoid.

### Two proposed fixes, both measured, both wrong

**"Score only frames where the player had ≥2 legal moves."** Measured over 60
games per opponent: that predicate fires on **99.55%** (wall-follower) and
**99.78%** (chaser) of frames. Trails retract, so the arena stays ~90% empty
and having two options is the normal state, not a junction. The filter filters
nothing.

**"Also require a blocker close ahead (runway ≤ 6)."** Measured: **1.57%** of
frames against the wall-follower, **18.08%** against the chaser — an 11× swing
driven purely by playstyle. A metric whose sampling rate is a function of how
the human plays is not comparable between humans, which disqualifies it for
the audience this is built for.

### The real defect: uniform chance is the wrong null

Both proposals kept a uniform-choice baseline (~33%, since a reversal is never
legal, so it was never the 25% the UI hardcoded). Measured against the chaser,
the player goes straight **98.4%** of the time. A model that always predicts
`Straight` therefore scores ~98%, and against a 33% baseline that reads as
*"read rate 98% vs chance 33% — it has found a pattern."*

It has found nothing. The vanity metric, reintroduced through the back door,
inside the very change meant to eliminate it — and biased in the direction
that flatters the CPU.

`rps.shaal.dev` can use 33% because a human playing rock-paper-scissors really
is near-uniform; they are *trying* to be random. A human driving a worm is
not, and a null that ignores that is not a null.

## Decision

**The baseline is the player's own base rate**: the accuracy of always
predicting their commonest turn, over the same window.

```
lift = (rate - base_rate) / (1 - base_rate)
```

`lift == 0` means the model is worth exactly as much as assuming you do the
usual thing. `lift == 1` means every decision called. It is self-normalising —
a player who genuinely never turns produces a base rate near 1.0 and cannot
inflate the score by being predictable — and it **cannot be gamed by
predicting straight**, which is the property the number needs.

Supporting decisions:

- Moves are recorded as a **relative `Turn`** (Straight/Left/Right), not an
  absolute direction. A habit like "breaks left when cornered" is one pattern
  in turn space and four unrelated ones in direction space.
- **Uniform chance is still reported**, computed exactly as the mean of `1/k`
  over the decisions actually faced — it is what the reference shows and it is
  meaningful — but significance is judged against the base rate.
- **No rate is displayed below 30 samples.** A three-for-three start must not
  read as 100%.
- The every-frame counters are **left untouched**, so existing telemetry does
  not silently change meaning.
- `lifetime_read` is `#[serde(skip)]` and rides in its own WRM2 section
  (`SEC_READ_RATE = 6`). `bincode` is not field-tolerant: a serialized field
  added to `CpuBrain` would break the legacy WRM1 path and cost a returning
  player their entire corpus in the release that added a metric.

## Consequences — the metric immediately indicts the model

Run against **Lefty**, a scripted opponent that breaks left 85% of the time
when it has a real choice, 40 games with a persistent brain:

| bucket | samples | rate% | usual% | **lift%** | corpus |
|---|---|---|---|---|---|
| 1 | 4,542 | 97.9 | 98.4 | **0.0** | 4000 |
| 2 | 9,447 | 98.6 | 98.4 | **12.2** | 4000 |
| 8 | 26,201 | 98.3 | 98.3 | **0.0** | 4000 |

turn mix taken: straight 25,747 · left 331 · right 123

The CPU scores **98.3%**. "Always predict straight" scores **98.3%**. Lift is
**zero**, and `is_significant()` returns false.

The old metric would have reported 98.3% and looked like a triumph.

The signature is demonstrably present in the data — left:right of 331:123 is
the 85%-left habit — so the information is there and the model cannot use it.
Lift also *decays* with more data (12.2% → 0%), and the corpus is pinned at
its 4,000 cap from the first bucket, confirming the ~2-round memory window.

This is the root cause proven end-to-end in the product's own number: the
model predicts absolute direction, ~95% of frames are "straight", so the
argmax is permanently "straight" and a habit about *turning* has no channel to
reach a decision. Retargeting the model itself to `Turn` is the next
milestone; this ADR delivers the instrument that will judge it.

## Verification

`cargo test` — 92 tests pass (from 87). Five added:
`predicting_the_usual_thing_earns_no_lift` (the load-bearing one — a trivial
predictor with a 98% hit rate must score zero lift),
`calling_the_turns_earns_lift`, `uniform_chance_never_assumes_four_options`,
`read_rate_survives_persistence`, and
`a_brain_without_a_read_rate_section_restores_clean` (a pre-existing blob must
restore with no partial-restore warning).

Probes, both reproducible: `scratchpad/dp_probe.rs` (decision-point density,
option mix, runway thresholds) and `scratchpad/readrate_probe.rs` (the Lefty
curve above).

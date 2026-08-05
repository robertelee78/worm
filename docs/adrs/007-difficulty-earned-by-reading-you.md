# ADR-007: Difficulty Is Earned by Reading You, Not by the Clock

## Status
Implemented

## Date
2026-08-05

## Context

The product owner's target for the difficulty curve was explicit:

> 5-10 matches should feel harder, 15+ should feel impossible

and just as explicitly, on what must drive it:

> measured read rate. It gets better as it learns you... not arbitrary round
> counts.

The codebase had a `difficulty` field computed as `time / 300 + 1` — an
arbitrary clock ramp — and **read nowhere**. It was the wrong signal, and it
was not even connected.

Meanwhile the opponent model had no path to the wheel at all. `cpu_decide`
read `cpu_telemetry.scored`: the forecast for the frame *already in progress*,
whose true answer is sitting in `cycles[0].direction` by the time the CPU runs.
The prediction it steered on was a restatement of an observable, so no
improvement to the model could ever have changed a decision. The forecast that
genuinely targeted t+1 was built at the *end* of `update()`, ~160 lines after
`cpu_decide` had returned.

## Decision

**Forecast before deciding.** Learning and forecast production move ahead of
the CPU's decision. Safe there: the player has already moved, and the block
consumes only frame-start captures. `cpu_decide` now reads `next_forecast`.

**Difficulty is lift, not accuracy.** Raw accuracy cannot drive difficulty:
most moves are "keep going", so a model scoring 90% may have learned nothing,
while 45% against a 33% baseline is a strong read. `read_rate` is lift over the
player's own base rate, recomputed once per round and held constant for its
duration — a CPU whose aggression drifts mid-round reads as erratic rather than
adaptive. `difficulty` survives as a HUD tier derived from it, so the number
the player sees and the aggression the CPU spends are the same axis.

**Confidence buys commitment, never safety margin.** `hunt_floor_cells` scales
only the floor an *optional aggressive deviation* must clear:

```
spend = 0.55 * read_rate^0.7
hunt_floor = max(escape_floor * (1 - spend), ESCAPE_MARGIN_CELLS)
```

`escape_floor_cells` — the survival floor — is untouched at every read rate,
and the threat-dodge, ring-evacuation, forced-move and wall-follow layers never
consult the hunt floor. A well-read player faces a CPU that commits to
intercepts it would otherwise decline. They never face one that suicides. The
floor cannot drop below the flat manoeuvring allowance, so it cannot be tuned
into recklessness.

**`refresh_read_rate()` runs at every round boundary and after every brain
restore** — in both clients. Missing the restore call would reset a returning
player's difficulty to tier 1, which is precisely the opposite of the premise.

## Consequences

Measured against `Lefty` (breaks left 85% of the time when forced), 40 games,
one persistent brain:

| bucket | read_rate | tier | record |
|---|---|---|---|
| 2 | 0.569 | 3 | 5-0 |
| 3 | 0.676 | 4 | 5-0 |
| 4 | 0.760 | 4 | 4-1 |
| 6 | 0.782 | 4 | 4-1 |
| 8 | **0.804** | 4 | 5-0 |

Final: CPU 37, player 3. The read rate climbs with experience and carries the
tier with it — the learning curve is now a measured quantity rather than a
claim.

Moving the forecast ahead of the decision also improved play on its own terms:
win rate against the `chaser` opponent went 8.0% → 11.5%, and habit-frame
counts on both personas roughly doubled (the CPU survives longer against them
because its reads now inform its moves).

### On the plateau

`read_rate` settles around 0.80 rather than climbing indefinitely. That is the
model reaching the limit of what its current features can express: it holds a
single global turn prior, so it learns *"this player breaks left"* but not
*"this player breaks left when the wall is three cells away and the CPU is
behind them"*. The remaining headroom is contextual features, not more data —
the corpus is full by round 3.

This matters for the "15+ feels impossible" target. A plateau at 0.80 lift is
a strong read, but pushing it toward 1.0 requires the model to learn from
richer things, not to be given a steeper ramp. Compensating with an arbitrary
difficulty multiplier would restore exactly the clock ramp this ADR deleted.

## Verification

`cargo test` — 99 unit + 3 persona tests pass. The persona suite's null control
(`coinflip`, z = −0.3) confirms the CPU still cannot read an opponent with no
habit, so the climbing read rate is a measurement rather than an artefact.

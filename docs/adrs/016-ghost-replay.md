# ADR-016: Ghost Replay — The Real Human Becomes the Benchmark

## Status
Implemented

## Date
2026-08-06

## Context

Every fitness number in this repo — the domination suite, the persona
probes, the nightly Darwin sweep — measures the CPU against scripted
opponents. The product's own success metric is domination of the REAL
human, and their actual play evaporated when each round ended. ADR-013
named this the flywheel's missing edge.

## Decisions

**Rounds are reproducible by construction.** `restart()` (and round 1)
reseeds the RNG from a fresh per-round seed drawn from the parent stream —
whole sessions stay a pure function of the launch seed, and every round
becomes independently replayable from its own (seed, size, input log)
triple. The engine records both worms' executed direction changes and fire
frames (~1-3 KB per round).

**Two RNG streams.** CPU decision noise (explore rolls) draws from its own
`cpu_rng`, split from the world stream (food, power-ups, disguises). This
is what makes ghost replay exact: a replay drives both worms from the log
and never calls `cpu_decide`, so the world stream must not depend on how
many draws the CPU's thinking consumed.

**The contract is a test.** `test_ghost_replay_reproduces_a_recorded_round_exactly`
records a full round — turns, fires, autopilot CPU, item stream — and
asserts the replay reproduces winner, frame count, food, bombs, and both
final bodies bit-for-bit.

**The browser exports the evidence.** Every saved round now carries its
ghost log; the EXPORT MY ROUNDS button downloads the full history as JSON.
The data is the player's own, stored where their brain already lives.

**`ghost_eval` closes the loop.** `cargo run --release --example
ghost_eval -- worm-rounds.json` replays the exported rounds
chronologically with `shadow_learning` on: the REAL pipeline — episode
recording, ensemble scoring, sealed forecasts, the honest McNemar-gated
read metric — watches the recorded human exactly as it would have live,
while never steering. Output: per-round and lifetime lift against that
human's own base rate. Any candidate temperament evaluates the same way
via `WORM_TUNE_*`, which means the nightly Darwin can eventually rank
candidates by how well they read the owner, not just how they fare against
scripted personas.

## Costs, accepted

Per-round reseeding and the stream split reshuffle every fixed-seed
number in the repo — a one-time re-baseline, taken deliberately:
COLD 27-3 (90%) / WARM 30-0 (100%) lift 84%, habitual 37-3 (92%),
129 tests green. The warm arm remains undefeated across the re-baseline.

## What this unlocks next

Once the owner has played a batch of rounds on the v9+ build and exported
them: (1) the learning curve against the real human becomes a plottable,
regression-testable artifact; (2) `ghost_eval` joins `darwin.py` as a
second fitness axis — candidates must read the owner at least as well as
the champion before win-rate even gets a vote.

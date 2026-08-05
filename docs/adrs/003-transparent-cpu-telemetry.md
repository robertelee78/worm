# ADR-003: Truthful, Player-Facing CPU Telemetry

## Status
Accepted

## Date
2026-08-04

## Context
The browser CPU Brain already exposes more live model data than the reference
`rps-ai` game page, but several values have different scopes or meanings:

- Per-model scores and hit rates reset each round; the headline prediction
  accuracy persists with the serialized brain across rounds and sessions.
- The selected ensemble model predicts the player, but safety, item, intercept,
  and wall-follow layers decide the CPU's final movement.
- The k-NN model receives a hidden +0.15 selection bonus when warm, so the visible
  raw score can disagree with the selected predictor.
- Browser P1 and P2 "scores" are asymmetric: P1 is food value, while P2 is food
  value plus successful survival frames.
- Counterfactual predictions for all seven models are recorded every frame but
  not exported to the UI.

The reference project's persuasive strength comes from explaining its competing
models and showing aggregate evidence. Adding more unlabeled counters would not
create the same effect; the telemetry must explain what the CPU predicted, what
it actually did, why it did it, and whether the prediction was correct.

## Decision

1. Preserve benchmark-facing cycle scores, but add symmetric per-player food
   counters for browser and terminal presentation.
2. Track prediction accuracy at two explicit scopes: current round (owned by
   `WormGame`, reset on restart) and lifetime (owned by the persisted `CpuBrain`).
3. Score the previous active prediction in the game loop before refreshing the
   ensemble, including cold-start frames.
4. Record the previous prediction, actual player direction, and hit/miss result.
5. Instrument the final CPU movement with a `CpuDecisionReason` distinct from
   the ensemble's `prediction source`.
6. Export all seven pending model predictions, raw quadratic scores, effective
   selection scores, hit/sample counts, k-NN warm-up progress, memory lifetime
   observations, direction habits, and the CPU's projected player path.
7. Present round accuracy against the four-direction 25% chance floor and retain
   lifetime accuracy as secondary evidence with its sample size.
8. Store round history using symmetric food, round prediction evidence, death
   cause, action reason, and memory growth rather than incomparable cycle scores.

## Test Contract

- Every active prediction is scored exactly once against the next actual move.
- Round counters reset on restart while lifetime counters and memory persist.
- The JSON contract distinguishes food, round accuracy, lifetime accuracy,
  prediction source, final action reason, warm-up state, and per-model forecasts.
- Constant-direction play clears the 25% chance floor within the existing test.
- The browser smoke test renders prediction source, action reason, scopes, and
  round history without breaking game/audio lifecycle behavior.
- Any `cpu_ai.rs` change passes Rust tests and the held-out behavioral benchmark.

## Consequences

- Positive: players can see the live learning loop rather than infer it from
  opaque score markers.
- Positive: displayed comparisons are symmetric and every percentage has a
  scope and sample count.
- Positive: model selection and final movement no longer appear to be the same
  decision.
- Tradeoff: the WASM state payload grows modestly; it remains far smaller than
  the grid and particle data already sent each animation frame.

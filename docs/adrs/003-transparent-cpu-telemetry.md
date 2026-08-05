# ADR-003: Truthful, Player-Facing CPU Telemetry

## Status
Implemented

## Date
2026-08-04

## Updated
2026-08-05

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

A follow-up timing spike found that the first implementation still mixed two
frames: the action and projected path came from the decision already executed,
while the highlighted source, prediction, confidence, and model forecasts had
already been refreshed for the next frame. A model switch could therefore make
one panel internally contradictory. Round history also existed only in the DOM,
the WASM state was assembled by hand without a schema version, and the benchmark
continued to label the CPU's survival-weighted cycle score as "food."

The reference project's persuasive strength comes from explaining its competing
models and showing aggregate evidence. Adding more unlabeled counters would not
create the same effect; the telemetry must explain what the CPU predicted, what
it actually did, why it did it, and whether the prediction was correct.

## Decision

1. Preserve internal cycle scores for gameplay compatibility, but use symmetric
   `food_eaten_by` counters everywhere a benchmark, browser, terminal, or round
   history claims to show food.
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
9. Capture an immutable decision-time snapshot before `cpu_decide` acts: frame,
   prediction source, predicted direction, confidence, action reason, and the
   projected path it consumed. Export the post-frame ensemble separately as the
   explicitly labeled next forecast.
10. Replace handwritten state JSON with a versioned, `serde`-serialized DTO.
    Browser code rejects unsupported schema versions instead of silently reading
    a drifting field layout.
11. Persist bounded per-device round summaries in IndexedDB alongside the brain.
    Retain aggregate model hit/sample evidence across reloads and new matches;
    local corruption degrades to an empty history, never a failed game boot.
12. Make the behavioral benchmark pair naive/adaptive seeds, derive outcomes
    from `winner`, report round frames and actual food separately, and fail
    closed unless adaptive beats naive on held-out wins. No behavior constant is
    tuned as part of the reporting correction.

## Test Contract

- Every active prediction is scored exactly once against the next actual move.
- Round counters reset on restart while lifetime counters and memory persist.
- The JSON contract distinguishes food, round accuracy, lifetime accuracy,
  prediction source, final action reason, warm-up state, and per-model forecasts.
- Decision fields all name the same executed frame; next-forecast fields are
  separately labeled and may name a different model.
- The state contract carries a supported schema version and is serialized by a
  typed DTO with a Rust contract test.
- Constant-direction play clears the 25% chance floor within the existing test.
- The browser smoke test renders prediction source, action reason, scopes, and
  round history without breaking game/audio lifecycle behavior.
- Browser history survives a simulated reload and produces aggregate model
  evidence from bounded, validated records.
- A clean checkout passes `cargo check --all-targets`, including auto-discovered
  examples; obsolete diagnostics cannot hide behind local deletions.
- Any `cpu_ai.rs` change passes Rust tests and the held-out behavioral benchmark.

## Consequences

- Positive: players can see the live learning loop rather than infer it from
  opaque score markers.
- Positive: displayed comparisons are symmetric and every percentage has a
  scope and sample count.
- Positive: model selection and final movement no longer appear to be the same
  decision.
- Positive: a panel snapshot cannot combine evidence from two different frames.
- Positive: the evidence story survives reloads and the wire format has an
  explicit compatibility boundary.
- Tradeoff: the WASM state payload grows modestly; it remains far smaller than
  the grid and particle data already sent each animation frame.

## Proof (2026-08-05)

- 70 Rust unit/integration tests pass, including schema-version, distinct
  decision/next-forecast sources, exact forecast scoring, restart scope, brain
  persistence reset, and round-boundary resize contracts.
- The browser smoke test exercises the versioned schema, tactical action,
  scoped accuracy, safe DOM history rendering, audio lifecycle, durable match
  history, and boundary sizing. The real-Chrome gate proves IndexedDB history
  and brain restoration survive a page reload.
- `cargo check --all-targets --all-features` and clippy with warnings denied
  pass from the feature worktree. Obsolete auto-discovered examples are gone.
- The paired-seed held-out benchmark passes fail-closed: adaptive wins 99/100
  versus naive 0/100, with +19.4 round frames and +0.1 actual food. No behavior
  constant changed.
- `cargo llvm-cov --lib --tests` measures 77.49% line coverage overall and
  82.16% for the new typed browser-state module. AQE SAST reports zero
  vulnerabilities.

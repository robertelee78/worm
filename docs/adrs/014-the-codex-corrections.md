# ADR-014: The Codex Corrections — What Survived Measurement

## Status
Implemented

## Date
2026-08-05

## Context

An adversarial codex review of the ADR-012 batch returned seven ranked
findings (two High on the learning stack). Under ADR-010's
one-change-one-measurement rule, each was implemented and run through the
gauntlet — and this ADR records the full ledger, because two of codex's
theoretically-correct recommendations measured WORSE than the defects they
fixed, and the reverts are as load-bearing as the fixes.

## Adopted

**Hysteresis made real (High).** `intent_family`'s errand commitment
evicted itself every frame: the route field never enters occupied cells,
so the distance check at the player's own head read "unreachable" and
re-shopped. Route distance at an occupied cell is now `1 + min(adjacent
field)`. The persistence test walks a player past a Manhattan-nearer decoy
across four real moves and the commitment holds. En route the test caught
a second defect: re-shopping picked the Manhattan-nearest target even when
the player was demonstrably walking AWAY from it, flapping
commit/evict/commit forever — recommit now prefers targets the player's
last observed move actually approached.

**Silent models can no longer drive (High).** Selection happens AFTER
legal masking, among models that currently speak (`select_active`); a
silent forecast carries zero HUNT confidence. Measured refinement: the
confidence gate is SPLIT — `track_conf` (historical) still opens the
defensive CloseEvasion dodge on silent frames, because dodging an
extrapolated path is conservative, while `pred_conf` (zero when silent)
gates the intercepts, because aggression without a read violates the
product contract.

**The HUD ranks by the real selection quantity (Medium).**
`ensemble_rank_score` exported the retired quadratic score with an
additive bonus — a different ordering from the fixed-share weights that
actually decide. It now returns exactly what `select_active` ranks by.

**Projection realism (Medium).** `count_open_space_excluding` returns 0
when the destination itself is a predicted player cell (it previously
counted it, ignoring the exclusion for exactly the most dangerous cell),
and CloseEvasion excludes only the projected cells the player's trail can
actually hold at once (the suffix of length `min(len + growth, horizon)`)
— a length-2 player can no longer hallucinate a sealed pocket.

**Web persistence honesty (Medium).** A dead brain store now SAYS so
("memory unavailable — this session will not be remembered") instead of
impersonating a fresh player; a post-timeout IndexedDB success closes its
unreachable connection; the wasm-bindgen glue import is versioned with the
rest of the bundle.

## Tried, measured, REVERTED

**Specialist-Hedge sleeper charging.** Charging an abstainer the awake
population's average loss is the theoretically fair sleeping-experts
update — and it collapsed the power-up persona's voluntary-turn read from
74% to 30%: the intent models sleep on most frames BY DESIGN, and the
charge decayed their weight with the crowd so awake skill could never earn
rank. Silence is once again free; the sleeper-takeover risk codex
identified is closed by post-mask selection instead (a well-ranked sleeper
holds its weight but can never drive while silent).

**DirectIntercept strict reachability.** Requiring arrival by the forecast
horizon (`dist ≤ frames_ahead + 2`) made the intercept "truthful" and made
the game worse: browser-board wins fell 88.8% → 81.2%, head-to-head
distance ROSE, and long corner dwells returned. Moving toward a predicted
crossing is engagement pressure even when arrival is a beat late — the
trail laid en route still closes lanes. Camping stays prevented where it
was measured: CornerIntercept's win-the-race check.

## Honest scoreboard

Primary gate (fixed-seed domination): **best state yet** — COLD 26-4
(87%), WARM 30-0 (100%), lift 86%; habitual 37-3 (92%). Browser-board
probe: 197/240 (82.1%), at parity with the pre-ADR-012 baseline (83.8%)
and below the 88.8% intermediate peak — a peak partly fueled by the
silent-model leak's unearned aggression, which is exactly what this batch
removes. Open question recorded for the flywheel: commitment-vs-reshop
tension (strict-router personas read a few points worse under real
hysteresis; a possible third twin axis, to be measured, never assumed).
128 tests green.

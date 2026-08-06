# ADR-012: The Two-Swarm Batch — Errand Twins, Kinematic Traps, and Honest Silence

## Status
Implemented

## Date
2026-08-05

## Context

ADR-011 shipped intent inference and a food economy, and the product owner's
instruction was to keep going until the objectives are *actually* met. Two
measurement swarms were dispatched against the ADR-011 build:

- **intent-read** validated the eat/hunt/arm models against goal-driven
  personas playing real games (five personas, three seeds, shadow-ensemble
  A/B of twelve candidate upgrades).
- **engagement** quantified the "sit and spin" complaint, hunted the
  remaining death modes with full frame traces, and audited iPhone Safari.

Their findings were specific enough to implement directly. Every change
below landed as its own commit with its own controlled measurement, under
ADR-010's one-change-one-measurement rule.

## Findings and decisions

### 1. Abstention is preserved (honest silence)

`mask_to_legal(None, …)` on a free frame fell back to the relative turn
prior — which is fed only at forced turns and has ~zero Straight mass
(measured 0.005–0.012) — so a model with nothing to say was force-fed a
TURN guess on frames that are ~95% straight, then scored on it. The `arm`
model abstained on 64.8% of frames (no power-up on the board), held 92.8%
raw skill when one existed, and was scored down to a 2.7% selection share
by guesses it never made. `None` now stays `None` on free frames;
`score_frame` skips silent models. Forced turns unchanged — there the habit
prior is the best estimator and every model publishes its answer.

### 2. Six intent models: the errand, in your travelling style

The greedy-Manhattan intent models agreed with real routing on ~93% of
frames, but the disagreement landed almost entirely on voluntary-turn
frames — the only frames that carry a decision. And no single tie-break
serves both a strict router and a committed human: BFS-with-strict-ties
read a human forager's voluntary turns at 12.2%, hold-the-line at 93.7% —
same field, opposite tie-break.

Decision: `{eat, hunt, arm} × {hold-the-line, weave}` — six models, each a
first-step prediction off a multi-source BFS field over the board the
player faces. `eat` includes the CPU's disguised mines (to the player those
ARE food). `eat`/`arm` carry observation-driven target hysteresis: commit
to a morsel while the player's own moves keep shortening the route to it.
`hunt` routes to the cells *adjacent* to the CPU's head — the head cell is
impassable, and aiming at it read a genuine hunter at 65.5% raw. The
fixed-share weights elect whichever travelling style this human uses.

Measured (in-tree probe, real integrated pipeline, 24-game personas):
voluntary-turn read — human forager 6.2% → 84.6% (`eat`/hold drives 91%),
strict-BFS seeker 59.5% → 80.8% (`eatW`/weave drives 84% — the correct
twin elected in both cases), powerup-seeker 25.2% → 73.9%, hunter
38.4% → 64.2%. NULL wall-follower control: all six intent models at 0.00%
selection share, weights at the floor. Cost: three shared BFS fields per
frame, ~0.1 ms against the 150 ms budget.

Rejected on the swarm's measurements: trajectory-inferred targeting (worse
than nearest-plus-hysteresis on every persona) and scoring models only on
McNemar-discordant frames (98.8% of frames are already discordant, effect
< 0.05pp).

### 3. The board as it is about to be (CloseEvasion floor)

CloseEvasion was the only deviation layer with no space term. Traced
length-61 death: ten frames hugging the player at distance 2, open space
3,565 cells, then 16 in ONE frame when their advancing trail sealed the
pocket mouth. A static floor cannot see that; the new primitive
`count_open_space_excluding(player's predicted next cells)` scores a
destination against the board it will actually have to survive in.
Candidates below the escape floor on that measure are rejected whenever
any candidate clears it.

### 4. A long worm's tail is not a wall (`tail_aware_reach`)

At length 60+ every remaining warm death was OwnTrail: the static flood
fill counts the CPU's own 100-cell body as permanent wall, fails the
escape floor on a perfectly safe wall-hug, and the "roomier" override
steered it INTO the coil. `tail_aware_reach` is a timed flood fill
(positions[i] becomes enterable at t = len − i + pending_growth) answering
Tron's classic question: can the head still reach its own tail? Applied
strictly as a RELAXATION of the base-policy floor (pass if the static
floor clears OR the tail is reachable) — never at CloseEvasion, where the
current-board fill cannot see the pocket the player's future trail is
about to seal. Measured on the frames where the floor binds: the
destination was actually survivable on ~30% of them (8–56% across seeds).
Result: the first zero-death sweep in the project's history (see numbers
below, pre-engagement-gates state).

### 5. The corridor pin is refused, not survived

The one deterministic loss a player could execute at will: escort the CPU
parallel one row inside a wall lane, diagonally abeam at equal speed — it
then has exactly one legal move per frame until the facing wall kills it.
Every traced case was at length 1–2, read 0.00: the COLD-START path. No
flood fill can see this trap; the sealed region is unreachable only under
the no-reversal rule, so the trap is kinematic and the defence is
geometric. `escorted_lane_step()` recognises the formation position
(player visibly parallel, ≤2 cells laterally, ≤2 longitudinally — farther
behind can never catch up at equal speed); an escorted step is filtered
like a projectile cell, in the cold-start, memory-driven, and base-policy
paths alike. Regression test builds the traced geometry and plays the
exploit out; verified to fail with the guard disabled. Information parity:
uses only the player's visible position and heading.

### 6. Engagement: hunt more, camp never

Measured at ADR-011: ~90% of CPU decisions self-referential, 6–7%
player-directed, head-to-head distance statistically indistinguishable
from two random points (ratio 0.91–1.08 vs uniform), and the long corner
dwells entered under CornerIntercept — which parked on the predicted
corner with no check that arriving mattered. Decisions: CornerIntercept
must be able to arrive strictly BEFORE the player (their straight-line
Manhattan distance is their arrival time; an intercept you can't win is a
camp), and the intercept confidence gates dropped (corner 0.5→0.35,
direct 0.6→0.45). `read_conf` still multiplies in — nothing opens before
~10 observed real choices — and `hunt_floor_cells` still vets every
destination, so game 1 stays a solid-basics opponent that hasn't read you
yet, and the added aggression is earned by the read (ADR-007).

Browser-board probe (55×40, 240 games, 3 seeds × habitual+forager):
wins 201/240 (83.8%) → 213/240 (88.8%); habitual blocks 80→100%, 85→98%,
88→92%; player-directed decisions up ~half again. Known remaining: rare
long dwells entered by SurvivalMemory (the warm self-memory reinforcing
wall-hugging — cold 15.1% near-wall vs warm 41.4%), recorded for a future
round.

### 7. HUD honesty

The escape-floor rescue reused ThreatDodge and claimed "dodging a weapon"
with no weapon on the board (nine consecutive frames in the traced
length-61 death). New reasons: `EscapeFloor` → "backing out of a dead
end", `LaneRefusal` → "refusing to be pinned along the wall". A label the
player can catch out is fatal to a HUD whose job is being believed
(ADR-003/ADR-006).

### 8. iPhone Safari (web)

No parse error anywhere — the breakage class was silent runtime failure on
a device with no console. Shipped: boot failures and unhandled rejections
now paint the actual cause into the cabinet; `indexedDB.open` handles
`onblocked`/`onversionchange` with a 3s timeout (iOS restores
prior-session tabs holding the old DB version — the v2→v3 upgrade blocked
forever and boot never finished); `touch-action: manipulation` on the
D-pad (rapid taps read as double-tap-to-zoom); the Google-Fonts stylesheet
loads non-blocking (a LAN-only phone with no DNS stalled first paint for
tens of seconds); `?v=` cache-busting on app.js and the wasm fetch, and a
schema mismatch paints "please reload" instead of freezing frame 1.

## Wire and persistence consequences

`ENSEMBLE_MODELS` 10 → 13. Slots 7–9 keep their historical indices; the
weave twins append at 10–12, so `KNN_MODEL=6` and the web round-history
validation (`models.length >= 7`) hold. The persisted ensemble section
migrates by WRM2 section-drop — scores re-earn within a session, the
opponent corpus and turn priors carry forward untouched. Intent
commitments are `#[serde(skip)]` transient: an errand does not survive a
session.

## Controlled results at this ADR

Fixed-seed suite (120×38, `tests/domination.rs`): WARM 29-1 (97%) ≥ COLD
28-2 (93%), lifts 79/89%; 40-game habitual 39-1 (98%), lift 75–81%.
After the survival changes and before the engagement gates the same suite
measured its first ever zero-death sweep (COLD 30-0, WARM 30-0 lift 83%,
habitual 40-0 lift 80%); the engagement gates traded three deaths across
100 games for the browser-board win-rate and engagement gains above —
accepted deliberately, ADR-009's warm ≥ cold invariant holding throughout.
123 tests green.

## The mantra check

No cheating: every new signal (positions, headings, visible plants) is one
the human also sees; the models learn only from observed moves. Initially
beatable: cold-start is unchanged wall-follow; every aggression gate is
observation- and read-scaled. Explainable: the explainer panel gained one
sentence per mechanism, including "an assumption with nothing to say stays
silent" and the two travelling styles.

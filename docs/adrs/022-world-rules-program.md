# ADR-022: The World-Rules Program — Two-Lane Corridor, Timed Decoy, Napalm

## Status
Accepted — v6 (corridor) in execution; v7 (decoy) and v8 (napalm)
design-gated on the interaction matrix below and the owner decisions
marked OPEN. Supersedes nothing; extends the ARENA_VERSION discipline
that v2–v5 established.

## Date
2026-08-07

## Context

The owner specified three world changes in one breath: the outer
corridor two lanes wide ("shrinking the arena a little"), the bomb
recast as a 15-second timed decoy that flashes at 13s/14s and detonates
at 15s punching walls, and the tri-shot recast as napalm (4-cell bolts,
3-second flames, a 5/3/1 burn schedule that kills through the head).

A first implementation landed all three behind a single `ARENA_VERSION
= 6` gate, without the kata's consult step. It is parked as a spike on
`feat/world-v6` (2717907) and produced real diagnostic value — and real
regressions. Both consultants (codex/GPT-5.6, ~600KB investigation;
kimi-k3) reviewed the spike, the failures, and the fixes, and returned
convergent verdicts. This ADR is their synthesis and the plan of record.

## Decision 1 — Unbundle: one physics change per world version

**v6 = corridor only. v7 = timed decoy. v8 = napalm.** Ordered by
increasing rule-novelty (pure geometry → new timer state → new per-cycle
worm state + new DeathCause). Unanimous across both consultants.

The spike is the empirical argument: its read-collapse was unattributable
among three simultaneous changes until a version-pinned A/B isolated
geometry, and its warm-arm regression turned out to be warm-only
machinery (exploratory mine plants) interacting with the *decoy* fuse —
invisible while everything shipped as one gate. Sub-flags within one
version were rejected: they buy a flag-matrix testing burden AND a
version byte that can no longer say which physics a replay ran.

Each version lands through the same ritual: physics contract → all
prior-version replays bit-exact → live-to-ghost identity on the new
version → same-frame/draw and failure-path tests → statistical
invariant suites → geometry/weapon benchmark receipt → only then may
the next version begin.

A `WorldRules` view derived from the version byte (`corridor_lanes()`,
`bomb_expiry_mode()`, `trishot_mode()`, `arena_base_offset()`) replaces
scattered `arena_version >= N` comparisons. It stays a *view*: one
serialized version byte, never independent mutable flags.

## Decision 2 — Re-baseline vs. invariant (the codex rule)

**Re-baseline measurements of the arena; never re-baseline measurements
of the learner.** Anything geometry measures may move, but only with a
paired receipt (same seeds, old-vs-new version, supply funnel attached):
win/draw rates, round lengths, corridor/lane occupancy, forced-turn and
two-lateral supply, weapon rates, and which seeds still supply a
geometry-coupled persona.

Invariants that hold regardless of geometry:
- NULL personas never earn read or projection authority — the null band
  is a pipeline property, not a geometry property.
- The family-wise anytime boundary, maturity floors, and coverage
  penalties do not change because geometry changed.
- Forced or single-lateral turns never become side evidence; frozen
  frames observe nothing.
- Warm-vs-cold non-inferiority ("memory costs no more than the declared
  margin") is a PRODUCT invariant: diagnose a failure, never
  re-baseline it away. Enforced per version.
- The novice envelope: competent basics, a visibly beatable opening, no
  false sharpness from time or geometry.
- Replay identity, deterministic RNG consumption, first-lethal/draw
  rules, exactly-once ledger finalization.
- "A learned habitual player is dominated" stays the product claim even
  if its fixture seed must change with a receipt.

## Receipts already banked (the spike's diagnostic yield)

Instrumented via a permanent `FunnelStats` counter set over the
evidence-eligibility path (`game.rs`) and version-pinned arms
(`play_v`, `tests/domination.rs`):

1. **The 60-game read collapse is look-schedule timing, not supply and
   not a pipeline break.** v6-code × v5-geometry at 60 games: lift 0.54
   (the historical value, supply 68). Same code, v6 geometry: lift 0.00
   at supply 70 and equal channel strength (z 4.84 vs 4.85) — the
   shifted trajectories put z under the boundary at each discrete
   geometric look. At 90 games v6 latches 0.52. The habitual arm is
   re-baselined to 90 games WITH this receipt.
2. **The warm-arm regression is not the read.** Seeds 20260805 and
   987654 dominate at v5 with family read 0.00 — warm superiority never
   depended on the read there — and 987654's v6 collapse (97%→60%)
   happens with the read unchanged. Seed 31337's read is healthy under
   both versions (0.70→0.60). The collapse tracks the *bundled weapon
   changes*, which v6-corridor does not contain.
3. **Cold-arm scrap deaths: the doze walked into its own mine.** The
   decoy fuse outlives a wall-follow lap; the pre-v6 fuse fizzled
   first. Fix (kept, consult-endorsed, to be encoded as a test): the
   doze wake-reflex treats OWN mines as hazards — self-knowledge, not
   sharpness — while enemy mines stay invisible to the doze, because
   being fooled by the disguise is what the doze is for. Cold arm 91%.
4. **Session doze-exit latch** (k3 Q6, adopted): once any earned read
   has ended the casual opening this session, the doze never returns.
   A latch that re-released on a marginal look-crossing was measuring
   "currently seeing crossing-shaped inputs", not "has earned
   sharpness". The latch never serializes — but it is not purely
   session-scoped either (codex verify round): every load path calls
   `refresh_read_rate()`, so a brain restored WITH live earned evidence
   re-latches immediately. That is the intended semantic: the ADR-018
   beatable opening belongs to UNREAD sessions — fresh brains, and
   returning humans whose read has genuinely lapsed to zero. Wits
   earned against this human do not lapse with the calendar.
   Aggression spend still tracks live evidence; only survival basics
   latch. Contract test exercises the real snapshot site, the release,
   and the wire boundary.
5. **Fixture limit, pre-existing:** the habitual persona supplies ZERO
   voluntary two-sided turns in either version (`vol_two_sided = 0`) —
   the class-book side gate never fires for it; its evidence rides the
   forced-break lateral channel (~1.5/game). A book-side persona is a
   test-suite gap, not a v6 regression.

## Decision 3 — The v7/v8 interaction matrix (design before code)

Specified now, coded only in their versions. Defaults below are the
consultants' recommendations; items marked OPEN are owner calls.

| Interaction | Ruling |
|---|---|
| Fuse clock | Wall-clock frames from plant (global frame count), never `frame_delay` and never per-worm active frames — a freeze must not disarm a mine, and a speed change must not retune the telegraph. Flash tiers derive from fuse-remaining. |
| Owner adjacent at expiry | Blast is owner-blind: standing next to your own expiring mine kills you. The counterplay is memory ("I planted that") — the same rule the doze already encodes. |
| CPU parity with the flash telegraph | The flash is a visual channel; the CPU/personas get the programmatic equivalent (fuse-age query) — information parity, no asymmetric-by-accident features. The browser must NOT paint a danger zone before the first flash tier (it would reveal the decoy). |
| Decoy vs. food spawner | Spawn exclusion: never place real food on a decoy cell or vice versa. |
| Flame on a decoy | Fire cooks the bomb: early detonation. |
| Burn while slipstream-frozen | Burns (the sticky 5/3/1 schedule runs on wall-clock). Consistency over mercy; slipstreaming into a flame edge is a real hazard. OPEN if play-testing reads as unfair. |
| Flame in the two-lane corridor | Blocks ONE lane, not the corridor; tests assert single-lane blocking. |
| Bolt "stop" definition | Wall-hit, 4-cell expiry, and worm-hit all ignite (a bolt into a worm drops fire under the victim). |
| Napalm owner immunity | OPEN — the spike made owners immune to their own flames without a decision. Symmetry with the mine ruling suggests owner-blind fire. |
| Burned → TriShot powerup drop | OPEN — it shortens the kill-feeds-weapon loop into a close-range brawl amplifier; alternatives are no-drop or Bomb. |
| Burn attribution | A `BurnState` (owner recorded at catch) replaces parallel arrays; `DeathCause::Burned` alone cannot attribute the kill for the weapon ledger. |
| Hazard phase | One common end-of-frame lethal-resolution phase; no early-return path may tick projectiles but not flames. |
| Sudden-death geometry | Recomputed from `arena_base_offset()`, not a hardcoded ring 2. |

## Consequences

- The corridor ships first and alone; the live site stays on the proven
  build until v6-corridor passes the full ritual.
- `pos_in_corridor` splits into physics membership (ring-based) and
  evidence/discipline membership (geometry-robust, "two open lateral
  exits") — the spike showed the two predicates only coincided by
  accident pre-v6 (k3 Q5.2).
- `set_world_version` narrows to a doc-hidden test fixture; production
  version construction stays on the recorded-round path.
- Head-on draws rise in a smaller arena (both arms tripled). Accepted
  as honest outcomes for now; OPEN whether corridor spawn headings
  should be co-directional if the draw rate annoys in play.
- The spike branch `feat/world-v6` (2717907) is kept as reference and
  never re-landed whole. Its "final step guard" in `choose!` was caught
  riding along by the verification round (k3 B1) and removed: it was
  v7-motivated (decoy-fuse deaths), untested, measured to change
  nothing on the arms it claimed to fix, and it misattributed tactic
  credit. If a computed-direction survival veto is ever wanted, it
  arrives as its own change with its own A/B receipt.
- The funnel gained `pend_dropped` (verify round, both consultants):
  pending book records produced on frozen-player frames are overwritten
  before any take, and restart() discards one — losses the take-side
  counters cannot see. `lat_supply` is documented as an upper bound of
  the published channel's feed (silent-model frames count toward it
  without feeding `lat_samples`).

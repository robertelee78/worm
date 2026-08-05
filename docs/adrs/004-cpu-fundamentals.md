# ADR-004: Fix the CPU's Fundamentals Before Teaching It Anything

## Status
In Progress

## Date
2026-08-05

## Context

A ten-agent audit of the CPU measured its play against scripted opponents and
found that it loses to a plain right-hand wall-follower. The product goal —
"the CPU learns the human player and gets smarter the more they play" — is not
reachable from that starting point. The product owner's framing:

> Broken fundamentals must be fixed. We want it to get better at beating the
> human player, but it can't be retarded by default. Solid basics that hasn't
> read you yet is correct.

So game 1 should present a CPU with competent basics that simply has not read
*you* yet — not a CPU that blunders.

### Baseline

Measured with a seeded harness (`winrate.rs`) that drives the real `WormGame`
loop, 200 games per opponent in 5 buckets of 40, one `WormGame` per run with
`restart()` between games so the brain persists exactly as in real play:

| opponent | CPU win% | CPU end-length | frames |
|---|---|---|---|
| wall-follower | **34.0%** | 2.0–3.6 | ~2100 |
| chaser | **7.0%** | 1.8–2.3 | ~300 |

The CPU ends games at length 2–3. In a snake/Tron hybrid where trail length is
the only area-denial tool, that is a cycle with no weapon.

### The defects

1. **The food route is computed and then discarded.** `cpu_decide`'s tier-3 BFS
   gated on `food_dir != wall_dir` — a HUD-honesty guard so a move would not be
   labelled `ItemPath` when wall-follow would have made it anyway. Measured over
   20 games: of 17,280 frames with a reachable food route, the CPU moved *away*
   from it 69.5% of the time, and **98.6% of those were frames where the food
   lay in the wall-follow direction** — i.e. the guard fired and the layer
   declined, letting the survival k-NN wander off instead.

2. **The CPU does not know its own length.** `grep "positions\|pending_growth"`
   over `cpu_ai.rs` returns nothing. `open_floor` is an *arena fraction*
   (`SURVIVAL_MIN_OPEN = 0.12`), so early game it demands ~1,824 reachable cells
   of a 9-cell snake (absurdly strict), and once total reachable space drops
   below 547 it silently disables *every* deviation layer at once (blind exactly
   when the endgame gets interesting). The quantity that matters — can I still
   escape given how long I am — is never computed.

3. **No sudden-death awareness.** `grep "shrink_level\|SUDDEN_DEATH"` over
   `cpu_ai.rs` returns nothing. Against a passive opponent this dominates: 47 of
   100 games end at exactly frame 3150 with the CPU standing on the ring
   `close_ring(3)` seals, because the right-hand wall-follow strategy hugs the
   inner face of the ring-2 wall — which *is* the first ring to close. Note this
   is scoped: a separate simulation measured 0 of ~1,500 matches reaching frame
   3000 against an *active* opponent, so this matters for long games, not most.

4. **Death reinforces the move that caused it.** `CpuBrain::remember` calls
   `observe(dir, reward)` unconditionally and `observe` does
   `tally[idx] += 1.0 + reward.max(0.0)`, so a death bumps the global prior for
   the fatal direction by the same amount a survival does. The k-NN *vote* is
   protected (crash episodes are zero-weighted) but the *prior* is not — and the
   prior is what blends in when memory confidence is low, i.e. exactly when the
   CPU is least sure.

5. **Bomb dodging starts too late to work.** `cell_threatened_by_bomb` is called
   with `frames_ahead = 3`, gating on `fuse <= 4`, against a fuse of 26–85 frames
   and a blast the CPU needs ~11 moves to escape. It has never dodged a bomb it
   was not already outside of.

## Decision

Fix all five before any learning work. Each change is measured independently
against the baseline above; a change that does not move the number is reverted
rather than kept on argument.

### 1. Commit to the food route — SHIPPED

Drop the `food_dir != wall_dir` clause and preserve HUD honesty by *labelling*
the coincidence (`WallFollow` when the directions agree, `ItemPath` otherwise)
rather than by forfeiting the food.

| opponent | before | after |
|---|---|---|
| wall-follower | 34.0% | **84.5%** (+50.5pp) |
| chaser | 7.0% | 8.0% (noise) |

CPU end-length rose from 2.0–3.6 to 4.3–6.4, and mean game length against the
wall-follower fell from ~2,100 frames to ~610 — the CPU now ends games instead
of orbiting.

### 2. Length-relative survival floor — SHIPPED (measurement-neutral)

`escape_floor_cells(game, who) = (positions.len() + pending_growth) * 3 + 8`,
in absolute cells, replacing the arena fraction at all three consumers (food
route, corner intercept, direct intercept).

**Win rate did not move — the per-bucket numbers were byte-identical.** The
hypothesis about *where* the old floor binds was wrong: on a mostly-empty board
`count_open_space` returns nearly the whole interior (~3,600 cells), so the old
~1,824-cell floor passed anyway. It only binds in constrained space, which
games now averaging ~610 frames never reach.

Kept anyway, as a correctness fix with unit tests rather than a win-rate claim:
the floor is now length-relative and counts owed growth, so it cannot demand a
whole arena of a three-cell cycle, and it cannot switch every deviation layer
off at once late in a round. Pinned by `escape_floor_scales_with_length_not_arena`
and `escape_floor_counts_owed_growth`.

### 3. Sudden-death ring evacuation — SHIPPED (proven by test, not by benchmark)

`WormGame::ring_seal_eta(x, y)` reports frames until the ring through a cell
seals; `sudden_death_max_level()` is extracted so the schedule has one
definition instead of being duplicated inline. The CPU excludes doomed cells
from its candidate set and, via `evacuate_ring`, from wall-follow itself.

**A first attempt failed its own test, which is how the real bug surfaced.**
The veto was applied at the candidate-filter level — but the cold-start
`WarmingUp` layer returns `wall_follow_decide` directly and bypasses every
filter below it. A fresh brain is always cold, so that is precisely the path a
first-time player sees. `evacuate_ring` is now applied at the cold-start layer
and the wall-follow fallthrough as well, so sudden death outranks every layer.

Not measurable on the current benchmark either: games end around frame 610 and
sudden death starts at 3,000. Pinned by
`test_ring_seal_eta_reports_the_scheduled_ring` and
`test_cpu_steps_off_a_ring_that_is_about_to_seal`.

## Consequences

Fixing fundamentals is a prerequisite for the learning work, not a parallel
track: 47% of terminal episodes were previously recorded against a death the
CPU's last move did not cause, which poisons the corpus with noise attributed
to good moves. A CPU that survives on its own merits produces a cleaner
training signal, so this milestone improves the learning even before the
learning architecture changes.

The `chaser` matchup is untouched by fix 1 and remains at ~8%. That is a
distinct failure — head-on collision discipline — tracked separately.

## Verification

`cargo test` — 77 tests pass after each change. Win-rate deltas are measured on
fixed seeds; per an independent replication, baseline variance across nominally
identical unseeded runs can reach tens of points, so only seeded comparisons are
reported here and deltas under ~10pp are not claimed.

# ADR-018: The Beatable Opening — Wits Are Earned, Not Given

## Status
Implemented

## Date
2026-08-06

## Context

Consistent feedback from the first wave of real visitors: "the other snake
is too good" — before it has learned anything. That violates the founding
PM contract (game one is a solid-basics opponent that hasn't read you;
dominance is EARNED by the read), and it starves the flywheel: crushed
first-timers don't come back, so nobody generates the ghosts the learning
needs. Quantified with a new fixture: a `novice` persona modeling a casual
human (5-frame attention, greedy food steering, one-cell lookahead, 10%
dither) won **2%** of rounds against the unread CPU.

## What did NOT work, measured

Boldness alone. The first attempt made the unread CPU reckless — thin
survival floors, eager extrapolation-chasing dives — on the theory that
aggression creates killable mistakes. Novice wins went DOWN (2% → 0%):
against a predictable novice, straight-line extrapolation is accurate, so
a bolder CPU hunts them better. Kept (it makes the opening exciting and
feeds the arc) but it is not the handicap.

## The lever that worked: sharpness

`WormGame::sharpness()` ∈ 0..1, from two PUBLIC inputs:
**the read** (it knows you) or **scoreboard pressure** (you are beating
it — losing focuses anyone). Fully sharp at 0.6 of either. Sharpness
drives, via tunable knobs (all Darwin-evolvable):

- **Decision latency** (`open_latency`, default 6): an unsharp CPU
  re-decides only every Nth frame — casual-human attention, not
  tick-perfect play. Held headings meet walls: genuine, killable
  mistakes. THE load-bearing knob.
- **Survival discipline** (`discipline_floor` 0.35): the unsharp escape
  floor is a fraction of the champion's.
- **Opening recklessness** (`bold_spend`, `bold_drive`): eager dives on
  raw extrapolation, fading as sharpness arrives.

Why pressure and not just read: an erratic human is nearly UNREADABLE
(the novice's lifetime lift stays ~0), and a read-only wake left the CPU
dozy forever — measured losing MORE the longer it played (68% novice wins
and rising). Scoreboard pressure closes that hole honestly: both players
can see the score, and "it wakes up when you're winning" is one sentence.

Ghost replays are unaffected (scripted rounds bypass the dozing branch),
and the CPU LEARNS at full speed even while dozy — the forecast pipeline
runs every frame. It is slow-witted, never blind.

## Measured, after unification

- Novice, fresh session, first five rounds: **3 wins, 1 loss, 1 draw** —
  the first-time experience the contract demanded.
- Novice, 40 rounds, one persistent brain: 45% overall with the CPU
  climbing to parity and beyond late in the arc, via pressure, against an
  opponent it cannot read.
- Fixed-seed domination: WARM 28-2 (93%) lift 81% vs COLD 19-11 (63%) —
  a **30-point memory gap**, the widest yet: the beatable opening stopped
  default strength from masking the learning. The COLD row's fall from
  ~90% is the feature, not a regression: an unread CPU is SUPPOSED to be
  beatable now, and ADR-009's warm ≥ cold invariant holds with room.
- Habitual warm: 35-5 (88%) — early dozy rounds cost a few games by
  design; dominance is earned per-opponent, as specified.

Behavioral tests that assert tick-perfect CPU conduct now pin
`read_rate = 1.0` explicitly — the formulas they check are the SHARP
values, which the opening deliberately withholds.

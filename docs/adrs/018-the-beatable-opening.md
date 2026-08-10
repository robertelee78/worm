# ADR-018: The Beatable Opening — Wits Are Earned, Not Given

## Status
Implemented (refined after owner play-test)


AMENDED 2026-08-09 (the concentration wake): the doze's wake list —
walls, own mines, one-step pockets, the ring — gains REAL COLLECTIBLES
within 4 cells. Receipt: a loop probe showed 14.7% of all frames spent
circling inside an 8x8 box, and the dominant mechanism was the doze
itself — a sampled controller re-deciding every Nth frame cannot make
cell-precise turns, so the dozy CPU orbited food it could never line
up, sometimes for whole rounds ("it just goes around and around" —
owner). Eating is a solid basic under this ADR's own contract; combat
sloppiness (trail blindness, thinned floors, held headings in the
open) is untouched, and disguised mines never wake it (food_items and
powerups never contain decoys by construction). Paired with the
survival floor's corner-tracking fix (a wall follower that turned
right whenever right was free was, in open space, a circler), loop
frames fell 14.7% -> 0.1% per 30-round probe. Doze contract fixtures
updated to clear incidental collectibles.
## Updated
2026-08-06 — the raw doze failed the look-and-feel test: a CPU holding
heading into a static wall reads as broken, not casual ("kind of
retarded. Runs into walls... doesn't play like it's trying"). Refined:
dozy frames keep WALL and sudden-death-ring reflexes (basics always on)
but stay TRAIL-BLIND — a fixated casual player rams trails mid-chase,
and dying into the trail YOU laid is the earned Tron kill the opening
exists to offer. A new low-sharpness Curiosity layer ("drawn to you")
closes distance while unsharp, manufacturing the encounters the
trail-blind doze converts into player wins, and fades out entirely with
sharpness. Novice fixture now ~25% wins + draws (instrument target 40-60%
stands open; the opening knobs are in the nightly Darwin's search space,
and novice win-rate is a candidate second fitness axis). Gates after
refinement: WARM 29-1 (97%) lift 84 vs COLD 28-0-2 (93%), habitual 36-3-1
(90%) lift 83.

## Amended
2026-08-06 (ADR-020 stage 1, both measured against warm domination arms):
Curiosity closes distance but never approaches down the player's own
driving lane (±1 cell ahead of their head) — the lane approach plus a
dozy held heading manufactured HEAD-ON draws, and the opening's earned
kill is the trail, not the kamikaze. The bold_* knobs scale by
`boldness_scale()`: full at first contact and even scores, fading to
zero as the CPU pulls ahead on the visible scoreboard — manufactured
recklessness is a first-contact affordance, and a CPU already winning
while unsharp does not need it.

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

# ADR-018: The Beatable Opening — Wits Are Earned, Not Given

## Status
Implemented (refined after owner play-test)

AMENDED 2026-08-10 (trail blindness ends; the lapse remains): the
2026-08-06 "stay TRAIL-BLIND" refinement below is superseded. Owner
reports from the arc video ("the cpu is kind of stupid in the 1st few
rounds -- runs into itself and the opponent more than expected";
"running into your own tail seems pretty silly"), grounded by a probe
and two convergent consults (codex + k3):
- OWN trail one cell ahead ALWAYS wakes the doze — strict self-
  knowledge, the own-mine rationale with more force (it laid every
  cell of itself this round). Post-fix an own-trail death requires an
  enclosure (no survivable option), never blindness. Probe receipt:
  the one residual OwnTrail death happened on a decided frame
  (decided=true — the CPU was awake and chose it, which cpu_decide
  does only under enclosure pressure), not on a doze coast.
- The opponent's live HEAD one cell ahead wakes — seeing a body coming
  is a basic reflex, not a read.
- ENEMY trail one cell ahead wakes IFF it is KNOWN SCENERY under
  DECISION-RELATIVE NOVELTY (codex): the cell already stood at the
  CPU's last real decision (last_cpu_decision_frame). A cell laid
  DURING the lapse stays invisible — the player whipping across a
  committed heading inside the CPU's reaction window remains the
  earned Tron kill (owner: "fair enough"). Parameter-free, and it
  dissolves at full sharpness where decisions run every frame. This
  is the human model the owner articulated: full board vision, finite
  reaction time — you die to the cut you couldn't react to, never to
  furniture you could see.
- Death-classification fix (probe audit, codex): driving off the board
  clamps the destination onto the boundary cell — usually the worm's
  own head marker — and misclassified edge deaths as OwnTrail. An
  out-of-bounds exit now classifies as Wall at both death sites.
- BALANCE REALLOCATION (codex warning, owner: "some sort of goldilocks
  region"): these wakes strengthen the unread CPU while novice
  non-loss was already ~35%, below this ADR's 40-60% target. The
  beatable-opening handicap must live in unread BOLDNESS (hunt
  commitment, intercept aggression, opening recklessness) — never
  again in navigation corruption. The novice fixture is the arbiter.
  MEASURED (novice_probe 40 games, seed 11), same-day:
  * post-wake baseline: novice 0% (was ~35%) — deaths 18 EnemyTrail,
    8 BombBlast, 8 Wall, 5 Laser, 1 OwnTrail. An UNREAD CPU was
    executing first-timers with lasers and mines.
  * UNREAD TRIGGER DISCIPLINE landed: should_fire holds every weapon
    while discipline_sharpness < 0.5 except the escape breach
    (survival, not aggression). Weapon deaths 13 -> 0; novice 5%.
    Aiming skill untouched (owner R3 intact — once sharp, every shot
    is exactly as lethal as before; precedent: dozed frames never
    fired).
  * UNREAD GREED landed (food destinations accepted at 0.45x the
    survival floor while unsharp): measured INERT against the novice
    (identical outcomes) — kept as doctrine (the honest casual-eater
    model) with zero measured cost.
  * open_latency default 6 -> 10 (sweep: 6=5%, 10=8%, 12=10% novice):
    a wider reaction window is more beatable AND more casual-looking,
    and opening rounds run ~27% longer, feeding the read histograms
    (k3's tempo warning: sub-10s rounds starve the learning the arc
    is supposed to show).
  * HONEST SHORTFALL: 8-10% novice non-loss vs the 40-60% target.
    The retired classes (stale-trail faceplants, head-on draws) were
    the old supply; knob-tuning cannot honestly replace them. The
    candidate mistake class is NAIVE-TRUST OVEREXTENSION — the unread
    CPU committing to steps whose escape assumes the opponent will
    not cut it off (the mistake every human novice makes). Queued as
    its own kata beside the exploitation-legibility work (F3); the
    40-60 target REMAINS OPEN, now with a named mechanism instead of
    a hope.
Contracts: test_doze_wakes_for_own_trail_and_player_head,
test_doze_enemy_trail_scenery_wakes_but_fresh_cut_kills,
test_oob_death_classifies_as_wall_not_own_trail.

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

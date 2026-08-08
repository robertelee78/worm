# ADR-023: Laser Simultaneity — The Beam Exists Across the Frame It Fires Into

## Status
Implemented and LIVE (world v7, merge a6e6c10, BUILD 24) — Option A
ruled unanimously, implementation verified through two adversarial
consult rounds (round 1: 4 findings, all resolved; round 2: SOUND with
two prescribed one-line completions, both landed with a contract test;
B2 truncation rule ruled Option 1 unanimously and written below). The
ADR-022 decoy and napalm are v8 and v9.

## Updated
2026-08-08 — landed live; owner's recorded round passes as the
regression test (his shot kills at frame 138).

## Date
2026-08-08

## Context

Owner bug report from live v6 play: a laser visibly crossed the
opponent's broadside and punched a wall breach, but nothing severed.
He was right. Frame-by-frame replay of his recorded round (round
frames=268, discharge at frame 137) shows the mechanism exactly:

```
frame 137: player (23,24) heading Left fires; CPU head (4,25) heading Up
frame 138 (the frame RENDERED to the player):
  24 #  OC=================Pppppp
  25 #  #c
  26 #  #c
  27 #  #cccccccccc
```

The discharge is evaluated between frames against the PRE-MOVE world;
the victim steps INTO the beam line during the very update in which the
beam is first painted; the flash then lingers ~20 frames. Lethality
lasted zero frames while the visual lasted twenty — the player shot at
what the game showed and was graded against a world it never showed.
Forensics over every recorded owner round show the same signature
everywhere: 11 misses by exactly one cell, 6 by two ("targets stepping
into already-dead beams"). The sever mechanic itself is correct — the
clean-room matrix passes and live replays show working cuts (29→2,
45→2, 41→3) whenever the beam intersects at discharge time.

This is the mirror image of the world-v3 bolt-ordering fix
("projectiles advance FIRST — they were fired in the past").

## Decision — DUAL TEST (codex Option A)

The beam exists across the one-cell movement transition. Two
evaluation points, one beam:

1. At discharge (unchanged): bomb detonations, head kill, body sever,
   breach — all computed against the trigger-time world, from the
   trigger-time origin and heading. Zero input latency; the aim the
   player took is the aim the game grades.
2. Post-move reconciliation (new, v7): after that same frame's
   movement resolves, re-test worm occupancy against the IMMUTABLE
   snapshotted beam cells. A head that entered the line is killed; a
   body cell that entered is severed at the crossing nearest that
   worm's head. Head-hit supersedes sever.

By construction, the first frame that paints the beam can never show
an un-hit intersection.

### Edge rules (codex, verbatim in substance)

- Beam path, origin, direction, ricochets, and breach snapshot ONCE at
  discharge. The reconciliation never retraces, re-breaches,
  re-detonates, double-counts, or re-credits: bombs detonate only in
  the first pass; breach is computed/applied/counted once; no second
  sound.
- Same-frame deaths are atomic: one survivor wins; both dead is a
  draw, regardless of the paired causes (laser, collision, bolt,
  bomb). No first-processed winner.
- The shot survives its firer's same-frame death — it was already
  discharged.
- The firer is immune to their own beam, head and body (a
  forward-moving shot must not be self-harm). Opposing simultaneous
  beams may kill both: draw.
- A slipstream-frozen worm enters nothing (it did not move) but
  remains fully hittable and severable by the discharge-time test.
- TRUNCATION RULE (B2, ruled Option 1 unanimously in verify round 2):
  a death truncates the movement transition at the point of death. The
  post-move reconciliation grades against the world as it stood at
  truncation — movements that resolved before the truncation are in
  the world; movements that had not resolved never happened. A worm
  whose movement never resolved cannot enter a line. This is the same
  player-first sequential resolution the head-on rule already encodes;
  the reconciliation inherits it, it does not create it. (Shadow-step
  and simultaneous-movement alternatives considered and rejected: the
  former grades the beam against a world the game never shows — the
  mirror of the lie this ADR closes — and the latter is a new world
  version, not an edge-case patch.)
- Accepted asymmetry (k3): the dual test is firer-generous — a target
  on the line pre-move that steps OFF during the frame still dies to
  the ignition test, though the painted frame shows no intersection.
  Deliberate: the alternative (step-off dodge) silently nerfs the
  laser and contradicts fire-frame intuition. Kept honest by the
  hit-marker rule below, never by weakening the test.

## Renderer contract (paired, same landing)

The visual says exactly what happened — lethal during the movement
transition, spent afterward:

- Frame 1: bright solid lethal core, drawn as full-cell quads over the
  exact sim beam cells (the render consumes the SAME cells array the
  sever test consumed — piped out, never recomputed in JS).
- Frames 2–5: rapid thinning/dimming afterimage.
- Frames 6–20: optional sparse embers only — visibly residue, never a
  solid beam (k3: solid == hot, faded == residue; the linger stays as
  a map-scar, legibly inert from frame +1).
- Hit markers anchor at the HIT CELL (mandatory, k3) — the ignition
  test's kills stay legible even when the victim stepped off the line.
- Impact/sever sparks and hit sound ONLY on true intersection; a beam
  that terminates on a wall without hitting gets a distinct clank and
  a wall-cell spark. Never a spark on an adjacent worm cell; no
  "graze" cue for near-misses (that would be aim-assist information
  the sim didn't act on).
- The half-cell particle anchor defect is fixed everywhere: particles
  draw centered in their cells (`px*CELL + CELL/2`), not at the
  up-left corner.

## Proof obligations (ADR-022 ritual, v7)

Physics contract tests for every edge rule above; the owner's actual
round replayed under v7 MUST sever/kill (the regression test IS his
ghost log); all v1–v6 replays bit-exact under their recorded versions;
live-to-ghost identity at v7; statistical invariant suites green;
benchmark receipts for any moved numbers, per ADR-022's
re-baseline-vs-invariant rule.

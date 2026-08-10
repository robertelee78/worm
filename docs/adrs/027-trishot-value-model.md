# ADR-027: Tri-Shot Value Model + Length-Dominance Boxing

## Status
Implemented 2026-08-10 (value model, trim-to-box transition, boxer
length-dominance entry, class census). Staging valve (k3's 2-step
horizon) reserved, not built.

## Date
2026-08-10

## Context
Owner: "the cpu doesn't really try to use the tri shot much" +
weapon_funnel_probe receipt: 56 fires / 0 lethal across 60 rounds
(laser: 18/26 warm) — the RCA F1 aim gate narrowed WHAT it shoots at,
never WHETHER the shot is worth a third of the napalm quota. Every fire
was a first-eligible-frame trail clip. Consults: k3 + gemini-3.1-pro
(codex out of credits until Aug 15), convergent on all classes,
divergent only on staging horizon.

## Owner rulings (binding)
R1 A short opponent (1-2 tail) must be killable by one tri-shot
   (burn-through; standing ruling, re-affirmed).
R2 The trim is a BOXING SETUP: "you can shorten the length of your
   opponent... it makes it easier to box them in because you have the
   length advantage post tri-shot."
R3 The Boxer must exploit length dominance: "it does not box in the
   opponent even though it sometimes has a length of four x the
   opponent" — grounded: boxer_choke_candidate gates on distance,
   suppression, and cornering; nothing keys on length.

## Decision
Fire classes (anything else HOLDS — the distant-tail-clip class is
dead by construction):
1. HEAD: aligned ray or v12 corner-brush to the opponent's head, per
   the existing per-version reach. Ungated — the only guaranteed-
   lethal class (R1's burn-through is its payoff).
2. BURN-TRAP (read-gated, earned_snapshot > 0): a bolt line covers the
   player's PREDICTED next-1..2 cells (straight extrapolation), or
   their wall-break-book exit when an imminent break is confidently
   predicted — napalm laid where they are ABOUT to be. Book-sourced
   traps respect the ADR-026 punish budget and emit the LearnedExploit
   receipt; open-field zone denial REJECTED by both consultants
   (spray-and-pray rebranded).
3. TRIM-TO-BOX (the A1 resolution): a strict-ray trail hit fires IFF
   severed >= max(4, their_len/3) AND the post-shot length ratio
   (self/theirs) >= 2.0 — i.e. the trim transitions the game into the
   boxing phase. Firing opens a TRIM-TO-BOX WINDOW (~20 frames) that
   feeds the Boxer. Below ratio 2, ADR-025's k3 counterweight dominates
   (a shorter opponent slips smaller gaps): hold. The laser trim rule
   is untouched (beam contact is guaranteed; bolt trims are
   commitments).
4. BOXER LENGTH-DOMINANCE ENTRY (R3): boxer_choke_candidate gains a
   dominance signal — live length ratio >= 2.0 OR an open trim-to-box
   window — which (a) bypasses the probe-suppression gate the way the
   recovery probe does, and (b) extends the engagement distance 14 ->
   20. Gated on discipline_sharpness >= 0.5: an unread CPU boxing an
   unpredicted opponent self-traps, and the beatable opening stays
   intact. Hysteresis, contested credit, and the shrink guard are
   untouched — dominance gets INTO the candidate set, never holds a
   choke by itself.
Cold discipline: the ADR-018 blanket hold stays absolute for the
tri-shot (gemini: STRICTER is right — a missed cold CQC fire dumps
napalm into the CPU's own maneuver space).
Staging: NOT built this stage. Gemini's constraint adopted for later
(stage only when the opponent's options are already wall/tail-
constrained); k3's 2-step horizon is the reserved valve if the class
census shows head fires starving.

## Proof obligations (k3 + gemini bars) — MEASURED same day
- R1 fixture: GREEN (test_v13_burn_quota_and_head_burnthrough — the
  one-shot kill on a short opponent is world physics, already
  contracted).
- CLASS CENSUS: GREEN — 30-round probe: cold 8 head + 13 trap + 0
  noise; warm 15 head + 10 trap + 0 noise. The noise class is
  extinct by construction.
- Anti-Goodhart fires floor: GREEN (21 cold / 25 warm >= 5).
- Lethality >= 15% vs the menace: NOT MET, and ruled
  instrument-limited (the instrument-ceiling doctrine): the menace is
  long, fast, and always eating — burn-through cannot kill it by
  physics, so its funnel can never show tri-shot kills. What the value
  model DOES convert on this fixture: NAPALM ATTRITION 90 (cold) / 70
  (warm) player segments burned off per 30 rounds, ~3 per fire —
  versus the old policy's pure noise. Kill-rate re-judged on video
  takes and owner play, where short-tail states actually occur.
- Trim-to-box census: 0 on this fixture (the ever-eating menace never
  presents a material sever + 2x ratio simultaneously); the class
  machinery is contract-proven
  (test_trishot_trim_fires_only_into_the_boxing_phase). Live frequency
  is matchup-dependent by design.
- Contracts: test_rca_trishot_gate_prices_the_shot (rewritten to
  ADR-027 — trail-clip class dead in every era),
  test_trishot_trim_fires_only_into_the_boxing_phase,
  test_trishot_burn_trap_is_read_gated.
- ADR-018 novice gate + full battery + zero clippy: see landing
  commit.

## Consequences
The tri-shot stops being a noise weapon: every discharge is a kill
attempt, a read-priced trap, or the opening move of a box. The receipt
makes trap kills attributable to the read (ADR-026's evidence chain),
and the Boxer finally spends the length advantage the owner watched it
waste.

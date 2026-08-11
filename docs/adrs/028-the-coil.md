# ADR-028: The Coil — a Deliberate Encirclement Kill Tactic

## Status
Implemented (stages 1-2, 2026-08-10): episode FSM, activation gates,
walls-as-ring-segments, closure receipt, existence contract + both
negatives. AMENDED same day: the ladder slot moved ABOVE the items
layer — the accepted below-items slot measurably abandoned a live
Tighten episode for grocery runs (fixture frames chose ItemPath with
a sealed pocket standing); survival layers keep their veto, and a boa
does not snack mid-squeeze. Remaining: abort/self-trap fixtures 5-6,
probe metrics, the on-camera wrap video.

## Date
2026-08-10

## Context
Owner, after the 40-round finishes video: "I still don't see evidence
of the cpu worm using its longer size to circle and trap an opponent.
That's a distinct kill tactic I'd expect it to learn." Grounded: the
Boxer (ADR-024/027) is a PER-FRAME choke heuristic — single steps that
shrink the player's region, credited afterwards. Nothing in the ladder
plans a wrap. The tactic cannot be seen because it does not exist.

## Decision (convergent design)
1. PLAN SHAPE: an explicit stateful episode — CoilEpisode FSM
   APPROACH -> CROSS -> CLOSE -> TIGHTEN (+ RECOVER on abort). The
   ring (boundary arc + gap) is computed ONCE at commit from the
   prey's region; per-frame work is waypoint-following + safety
   re-checks (O(1), wasm-safe). Greedy/potential-field variants
   rejected by both consultants (orbit-without-closure local minima;
   "per-frame" is the exact criticism).
2. ACTIVATION: length ratio >= 2.0, prey within ~10,
   discipline_sharpness >= 0.5, and PERIMETER FEASIBILITY — own
   length >= needed ring perimeter x margin (gemini: 1.25x; k3
   equivalent: prey region <= 0.75 x own_len). Ladder slot: above
   Boxer/intercepts, below reflexes/survival/items. An active episode
   short-circuits Boxer and intercepts; survival layers can veto any
   step (abort, never override).
3. TIGHTENING = STARVE, NEVER PRESS: once flood-fill confirms the
   prey's region is bounded entirely by CPU body + walls, trace the
   own inner perimeter (tail-follow). Being 2x+ longer shrinks the
   pocket every lap while the own moving tail guarantees the exit.
   SELF-TRAP GUARD: hold (WAIT) whenever tail margin < prey_len + 2;
   zero self-deaths is a hard proof gate.
4. LEARNING (owner: "a tactic I'd expect it to LEARN"): stable id
   tactic_coil in the ADR-021 ledger. Attempt booked at CROSS entry;
   KILL only when the prey dies inside the closed pocket during
   TIGHTEN (flood-fill confirmed); voids sub-counter (pre-closure
   suicide, escapes; ring-assisted deaths contested per existing
   doctrine). Suppression like Boxer: >= 10 attempts with decayed
   kill-rate < 0.15 parks it until the window refreshes.
5. FAIRNESS/LEGIBILITY: earned twice (read + length dominance);
   beatable three visible ways (punish the broadside cross; sprint
   the gap before closure; the ring is not closed until it is).
   ADR-026 receipt at closure ("coil: 3.1x length, ring closed,
   pocket 14 cells"); per-prey re-attempt cooldown ~150 frames after
   an abort so it never reads as harassment scripting.
6. PROOF: six fixtures — (1) positive closure with in-order phase
   transitions and in-pocket kill [FAILS TODAY — the regression test
   that the tactic exists]; (2) unread never coils; (3) ratio 1.5
   never coils AND Boxer still holds (no cannibalization); (4)
   infeasible region never activates; (5) gap-dash aborts to RECOVER,
   attempt booked, no kill, ladder resumes; (6) self-trap guard WAITs
   with zero self-collisions. Probe metrics per 30 rounds:
   attempts/kills/voids, mean closure frames, self_deaths (must be
   0), Boxer non-regression. VIDEO BAR: debug overlay of the ring +
   pocket, the receipt line at closure, and one recorded wrap
   end-to-end — the artifact that answers the owner's report.

## Consequences
The length advantage stops being decorative: the CPU that out-ate you
converts mass into a visible, narrated, learned kill — and the player
who watches the wrap close knows exactly which macro-game they lost.

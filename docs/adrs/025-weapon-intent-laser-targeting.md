# ADR-025: Weapon Intent — Laser Targeting, Symmetric Trigger, Shot Economy

## Status
Implemented (stages 1-4) 2026-08-09; stage 5 (offensive breach) gated
on the ADR-024 Phase-B competence battery pending an explicit owner
override. Owner rulings verbatim below; codex + k3 consults convergent,
both source-grounded. INCIDENT NOTE: commit a6e69e9 claimed stages 1-2
while the cpu_ai value-model hunks were absent from it (a lost-edit
during parallel patching); the follow-up landing re-applied them with
grep-verified receipts and passing contracts — recorded here per the
honest-history rule. This is CPU
POLICY, not world rules — beam physics (ADR-023 dual test, sever,
breach) untouched; ghost replays consume recorded fire events, so no
version gate (precedent: the v12 tri-shot aim-policy amendment).

## Date / Updated
2026-08-09

## Owner rulings (binding)
R1 The laser has THREE uses — headshot, tail trim (sever), wall breach
   — and the head-only gate was the ineffectiveness: the CPU could not
   even attempt trims or offensive breaches. ("Why does the CPU not use
   laser effectively?")
R2 "We don't need the dodge asymmetry at all": the 10-consecutive-frame
   hard-reset telegraph (2-3 completed fires per 90 rounds; ember
   flicker spam) is removed. Symmetric trigger discipline — the CPU
   fires the first eligible frame, like the player.
R3 Targeting is a tactic: movement cooperates with the held weapon
   ("lining up the shot"), predictive aiming is SKILL and is never
   gated on the earned read ("aiming is a basic" — read-gating
   marksmanship ruled unfair to the CPU).

## Decision (staged; each stage independently landable)

1. VALUE MODEL in should_fire (telegraph still on in this stage):
   - Headshot: always fire; unbounded reach including ricochets.
   - Tail trim: fire iff severed length >= max(8, ceil(0.5 x their
     length)) — a neck cut; tail-tip clips hold. Priced as
     BOARD-CLEARING and economy denial (sever_from deletes their laid
     trail cells), NOT as boxing setup: k3's counter-weight — severing
     shrinks their length-scaled escape floor, so a trim can make them
     harder to box. Threshold lives in tuning (laser_trim_threshold).
   - Breach, escape class: the cpu_enveloped rule unchanged.
   - Breach, offensive class: may be CONSIDERED (R1); LIVE offensive
     use remains gated by ADR-024 Phase B's competence battery unless
     the owner explicitly overrides here. OWNER OVERRIDE: [pending —
     asked 2026-08-09].
   - HOLD: no qualifying use = no fire. The weapon is scarce; the value
     model's whole job is trigger discipline.
2. SYMMETRY: delete the charge machinery — LASER_TELEGRAPH_FRAMES,
   cpu_laser_charge (all four sites; it is WormGame-transient, so
   replay- and WRM2-safe), the ember-charge render path, the telegraph
   test, and the ADR-001 telegraph contract (amended by this ADR; the
   ADR-024 landmine-5 "10-frame telegraphed commitment" note reprices
   to an unannounced instant commitment). The ADR-023 renderer contract
   is INVIOLATE: exact beam cells painted frame 1, hit markers at hit
   cells — fairness lives there now, plus tuning valves
   (laser_headshot_bounces as the first release valve if bounced
   instant headshots read unfair in playtest).
3. STEP-1 LEAD (tuning laser_lead): fire when the player's
   straight-ahead-if-passable next cell is on the beam — ADR-023's
   post-move reconciliation IS the lead mechanic (k3: the lead horizon
   is exactly 1 by construction; beams do not travel — a longer
   horizon fires at ghosts). Pure geometry, always on, no read
   coupling. Corner-turn leading is a read by definition and stays out.
4. TARGETING LAYER: CpuDecisionReason::Targeting ("lining up a shot"),
   placed after the hunts and before self-memory/wall-follow; 1-step
   rollout staging the next frame's fire check (movement at N stages
   the pre-move fire at N+1); may preempt hunts ONLY for an imminent
   next-frame fire-worthy beam; never preempts survival layers or a
   held Boxer episode; escape floor only (aiming is board geometry, not
   read-priced aggression); corridor entry filtered; Boxer-pattern
   hysteresis (hold the episode K=3, recompute each frame) with TTL 8
   frames (codex proposed 12; k3's 8 adopted — a weapon held is a
   weapon not fired) and a 15-point switch margin against
   target-flapping. NOT in TACTIC_IDS (wire-safe as a plain string;
   the fire remains weapon telemetry — never mix movement labels and
   discharge accounting).
5. OFFENSIVE BREACH under reserved actuator id 5, per the R1/Phase-B
   resolution above.

## Proof obligations
First-eligible-frame fire; trim threshold both ways; escape-breach gate
unchanged; step-1 lead three ways (on-beam fires / two-out holds /
blocked-ahead holds); targeting yield order + never-in-tactic-ledger;
zero charge-state ember frames across a full match; ADR-018 novice
fixture re-run AS A LANDING GATE (instant led lasers could quietly
kill the beatable opening); five-seed instrument; v1-v12 replay
bit-exactness; browser-wasm p99 with targeting on; weapon_ops
gate-pass/fire convergence assert (with no charge phase the counts
coincide — divergence means a double-count); telemetry receipt of
fires per use class, with NO lethal-kills target (Goodhart).

## Consequences
The laser stops being hoarded: it trims necks, deletes laid walls,
threatens breaches, and fires the moment a shot is worth it — while the
scarce-weapon discipline (hold when nothing qualifies) keeps it a
decision, not a spam. ADR-001's telegraph paragraph is superseded by
this ADR; ADR-024's breach-commitment note reprices.

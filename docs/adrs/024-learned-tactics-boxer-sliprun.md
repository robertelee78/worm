# ADR-024: Learned Tactics — Boxer, and the Breach→Slip-Run Ladder

## Status
Phase A IMPLEMENTED and amended by the 2026-08-09 intent RCA (below).
Phase B design-accepted, gated on the slip-run competence battery.
Original acceptance: owner directive 2026-08-08, k3+codex convergent.

RCA AMENDMENTS (2026-08-09, k3+codex convergent; swarm audit receipts —
120-game battery, Boxer 0 fires in 59k probe decisions but geometry
failure modes land exactly in close human play):
- FORWARD RELEVANCE: boxer_choke_candidate now requires the choke cell
  to sit in the player's PREDICTED forward half-plane (the ensemble's
  player_pred_dir, not the stale raw heading). The isotropic choke could
  seal a room the player was leaving, mid-chase — the owner's "swerved
  off the kill for no reason".
- EPISODE HYSTERESIS: a fired Boxer episode holds K=3 frames — the
  EPISODE, not a frozen direction; the safe choke is recomputed each
  frame and the hold releases on lost relevance/materiality or any
  floor veto. Higher ladder layers still preempt.
- RECOVERY PROBE (implements the §3 curiosity-floor promise, which was
  never landed): a suppressed arm keeps ONE Boxer start per round
  (boxer_probe_used, reset at the round boundary). CORRECTION: the
  previous "self-recovering via attempt-mass decay" claim was FALSE —
  decayed mass only moves on the arm's own attempts, so a silenced arm
  was frozen off forever. Recovery is earned by a realized choke via
  the probe, never by clock magic.
- ENGAGEMENT LEDGER (RCA F2a, transient, recording only): every tactic
  episode now records entry distance, read at open, and an exit class
  including the previously invisible "the CPU died trying" — the warm
  inversion's blind spot (warm 68% vs cold 88% with accurate reads).
  Conversion-gated hunting (F2c) stays OWNER-GATED pending this data.
- DWELL BREAKER (RCA F2b, the ADR-012 §6 corner attractor): the memory
  vote yields after 24 same-region votes, cooldown 48, releasing only
  on material improvement (>=1.5x wall-follow) or a shrink change.
  SIEGE EXEMPTION (same day, A/B receipted): pressure (player within
  10 cells) suspends the breaker and releases an armed cooldown — a
  boxed worm hammering its one escape region is fighting, not
  dwelling; with the breaker armed under siege an adversarial bot went
  14-0-1 with instant trail kills, with it suspended the CPU won 3
  rounds on its learned bait. The passive orbit the breaker targets
  only occurs unpressured.

## Date
2026-08-08

## Context

The owner watched the recorded Claude-vs-CPU session and named the gap:
"space-denial boxing isn't visibly in its repertoire — how can we ensure
it LEARNS this? I also want it to learn how to blow up holes in the
arena and enter the slip." The tactic ledger (ADR-021 Kata 0/4) has four
hunt arms; boxing, breaching, and corridor play are not among them.

A pre-consult spike over the frozen corpus (235 rounds / 52,173 frames,
examples/tactic_opportunity_spike.rs, receipts in
docs/spike-24-tactic-opportunity.txt) established:

- Natural boxer windows barely exist: tight setups on 0.25% of frames,
  in 4/235 rounds. An opportunistic arm would starve — boxing must
  CREATE its advantage, not await it.
- Zero of 116 player Wall/OwnTrail deaths had boxing geometry in the
  prior 10 frames: the incumbent tactics cannot produce these deaths,
  so any kill a boxer arm claims is net-new, clean attribution.
- The escape breach (should_fire, cpu_enveloped gate) has NEVER been
  eligible in real play: 5,816 CPU laser-held frames, zero enveloped.
  Humans slipstreamed 2,578 frames; the CPU has never entered the
  corridor. Any corridor tactic gated on envelopment inherits a gate
  that provably never opens.

## Consult record (both grounded in the repo, answers convergent)

Agreed: setup belongs INSIDE the boxer arm (shared setup state would
contaminate every arm's attribution); the 12-frame precommitted
attribution window stays — no 40/60-frame kill windows ("a long window
is a false-credit machine"); breach is an ACTUATOR inside slip-run, not
a standalone learned arm (the escape reflex stays as-is); slip-run
competence must be engineered and proven before the bandit may judge
it; choke and offensive slip are aggression spends funded only from
earned_snapshot (never the unread opening's bold_drive), and ADR-018's
novice fixture must be re-run; flood fills need caching and browser-wasm
p99 receipts; static flood space mis-values corridor cells (ignores the
16x movement cost).

Divergence resolved (trigger): k3 proposed static preconditions
(length + 1.2x space ratio); codex rejected the static ratio —
"tactical advantage is not evidence of a reachable choke" — and ruled
for a PROSPECTIVE test: a short rollout showing the candidate move
reduces the player's reachable space materially versus the incumbent
move. Decision: k3's perturbation shape + codex's prospective test.

Divergence resolved (slip-run v1): k3 said escape-only; codex said
deterministic/shadow planner first with a competence battery. Decision:
codex's ladder — an envelopment-gated tactic would never fire (spike),
and shadow planning builds receipts before risk.

Landmines recorded by the consultants, all binding on implementation:
1. The current "bandit" is a Thompson rerank of Direct-vs-Corner at
   round boundaries (game.rs ~2806) — appending ids does not create a
   multi-arm chooser; arms have different availability and reward
   semantics, so flat kill-rate comparison is selection-biased.
2. open_attempt = (id, opened_frame) cannot represent phases or causal
   baselines — an explicit episode record is prerequisite.
3. Holes are passable in EVERY flood consumer already; adding corridor
   tactics requires a corridor-leakage audit of count_open_space
   consumers or the bandit learns from entries it never chose.
4. Breach firing happens outside movement selection (should_fire, not
   cpu_decide) — weapon actions get separate telemetry, never a
   CpuDecisionReason move label.
5. The laser cannot punch an arbitrary hole: breach cell is determined
   by heading and the fifth wall strike; existing holes terminate
   beams. A breach is also a 10-frame telegraphed commitment
   (LASER_TELEGRAPH_FRAMES) — a boxable posture the plan must price.
6. Extending TACTIC_IDS runs the poisoned-wire sanitizer + golden brain
   fixture ritual (WRM2 discipline, ADR-021).

## Decision

### Phase A (this arc): Boxer + causal episode instrumentation

1. EPISODE RECORD (as implemented — narrowed from the original text
   after the k3 implementation verify; the full phase/close-reason
   struct is DEFERRED to Phase B, which actually needs it): the open
   window carries (tactic id, opened frame, precommitted baseline =
   player reachable space at open, shrink_level at open), all
   transient — the persisted wire shape is unchanged. Two credit
   guards landed from the verify: the SHRINK GUARD (a sudden-death
   ring closure collapses space mechanically and kills with
   DeathCause::Wall; a window straddling a shrink earns nothing) and
   the CONTESTED-CREDIT rule (a Boxer window closed by tactic
   replacement inside its horizon is re-tested at death against its
   own baseline and wins the credit over its replacement iff the
   choke realized — exclusive, never both; without this the terminal
   phase of a WORKING choke hands the kill label back to the
   intercept precisely because it worked). The 40-frame setup cap,
   per-round curiosity throttle, and per-frame flood cache from the
   consult text are likewise Phase B machinery, not implemented in
   Phase A — the prospective trigger's pruning made them unnecessary
   at current cost (wasm p99 receipt below).
2. BOXER (stable tactic id 4, CpuDecisionReason::Boxer): a conditional
   perturbation of an already-funded hunt. Trigger: an intercept-family
   hunt is active under earned authority AND a one-step rollout with
   the per-frame cached player flood shows the boxer candidate reduces
   player reachable space materially vs the incumbent candidate AND the
   CPU stays above its survival floor. Setup phase capped at 40 frames;
   choke phase minimizes the player's cached reachable space.
3. CREDIT: a realized choke (space collapse vs the precommitted
   baseline, still holding at death) opens the STANDARD 12-frame
   window; death cause must be Wall/OwnTrail/EnemyTrail; attribution
   exclusive; episode closes on rebound/replacement/abort/deadline.
   One curiosity-floor boxer start per eligible round.
4. IDS 5/6 RESERVED on the wire now (Breach actuator telemetry,
   SlipRun) — reserved, not active.
5. BUDGET: player flood computed once per frame and reused; candidate
   pruning before any flood; browser-wasm p99 receipt required.

### Phase B (gated): Slip-run ladder

SlipRun lands first as a deterministic SHADOW planner over existing
holes: plans logged with full predicates (two usable holes, legal
one-way corridor path under 16x time dilation, shrink ETA margin,
post-exit route above the escape floor, pursuit/beam/bomb/flame vetoes,
earliest-safe-forward-exit abort semantics), no behavior change.
Competence battery before ANY live entry: >=500 eligible geometries
replayed; zero entry-without-exit; zero planner-caused collision when
assumptions hold; >=95% planned re-entry; paired survival non-inferior
to staying in the arena; wasm p99 in budget. Live activation scores on
completed re-entry + relative survival, NOT kills alone; down-weighting
allowed only after 24 genuine entries; pre-entry aborts are opportunity
misses, not attempts. Offensive Breach becomes a slip-run setup action
(separate weapon telemetry) only after the battery passes. The
corridor-leakage audit of every count_open_space consumer is a Phase B
entry criterion.

## Proof obligations (Phase A)

- Episode/ledger unit tests incl. baseline precommitment and every
  close reason; poisoned-wire sanitizer + golden brain fixture green.
- A staged fixture where boxing IS available must produce Boxer
  episodes with realized-choke credit; the S2 result (0 accidental
  boxing) is the null control.
- ADR-018 novice fixture re-run: the beatable opening survives.
- Five-seed paired expected-score instrument: no regression.
- Browser-wasm p99 frame time within budget with the boxer layer on.
- Zero-warning build + clippy; bit-identical replay battery (v1..v11).

## Consequences

The CPU gains its first tactic that manufactures geometry instead of
exploiting found geometry, with attribution that cannot lie about it;
the corridor program gets an honest ladder instead of a dead gate; and
the tactic ledger gains the episode vocabulary every future arm needs.

WRM2 note (k3 verify, finding F): the persisted tactic section now
carries id 4 rows. An OLD binary loading a NEW save drops the ENTIRE
tactic section (ids 0-3 learning included) via the sanitizer's
unknown-id rule — the standing WRM2 subset discipline, recorded here
so a future downgrade surprise has a name.

Verification: k3 adversarial verify of the implementation returned
SOUND-WITH-FIXES (claims 1-7 confirmed with line receipts; full suite
independently reproduced; novice fixture intact at 8W/22L/10D). All
three mandated fixes landed before merge: shrink guard, contested
credit, and this ADR reconciliation.
